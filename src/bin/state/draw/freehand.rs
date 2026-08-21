use perfect_freehand::{
    InputPoint, StrokeOptions, TaperOptions, get_stroke_outline_points, get_stroke_points,
};

use super::scene::{Point, Style};
use crate::render::{FillRule, Geometry};

const CHUNK_POINTS: usize = 2048;

#[derive(Debug)]
pub(super) struct LiveStroke {
    points: Vec<Point>,
    style: Style,
    preview_start: usize,
    cached: Vec<Geometry>,
}

impl LiveStroke {
    pub fn new(point: Point, style: Style) -> Self {
        Self {
            points: vec![point],
            style,
            preview_start: 0,
            cached: Vec::new(),
        }
    }

    pub fn push(&mut self, point: Point) -> (bool, bool) {
        let changed = push(&mut self.points, point);
        let cached = changed
            && cache_ready_chunk(
                &self.points,
                self.style,
                &mut self.preview_start,
                &mut self.cached,
            );
        (changed, cached)
    }

    pub fn tail_geometry(&self) -> Geometry {
        build_chunk(
            &self.points[self.preview_start..],
            self.style,
            self.preview_start == 0,
        )
    }

    pub fn cached(&self) -> &[Geometry] {
        &self.cached
    }

    pub fn update_style(&mut self, style: Style) -> bool {
        self.style = style;
        if self.cached.is_empty() {
            return false;
        }

        self.preview_start = 0;
        self.cached.clear();
        while cache_ready_chunk(
            &self.points,
            self.style,
            &mut self.preview_start,
            &mut self.cached,
        ) {}
        true
    }

    pub fn finish(mut self, point: Point) -> (Vec<Point>, Style, Geometry) {
        self.push(point);
        let mut geometry = Geometry::empty();
        for chunk in self.cached {
            geometry.append(chunk);
        }
        geometry.append(build_chunk(
            &self.points[self.preview_start..],
            self.style,
            self.preview_start == 0,
        ));
        (self.points, self.style, geometry)
    }
}

fn push(points: &mut Vec<Point>, next: Point) -> bool {
    if points.last() == Some(&next) {
        return false;
    }
    points.push(next);
    true
}

fn cache_ready_chunk(
    points: &[Point],
    style: Style,
    start: &mut usize,
    cached: &mut Vec<Geometry>,
) -> bool {
    if points.len().saturating_sub(*start) <= CHUNK_POINTS {
        return false;
    }
    let end = *start + CHUNK_POINTS;
    cached.push(build_chunk(&points[*start..end], style, *start == 0));
    *start = end - 1;
    true
}

pub(super) fn centerline(points: &[Point], width: f32) -> Vec<Point> {
    get_stroke_points(&inputs(points), &options(width))
        .into_iter()
        .map(|point| Point::new(point.point[0] as f32, point.point[1] as f32))
        .collect()
}

pub(super) fn geometry(points: &[Point], style: Style) -> Geometry {
    let mut geometry = Geometry::empty();
    let mut start = 0;
    while points.len().saturating_sub(start) > CHUNK_POINTS {
        let end = start + CHUNK_POINTS;
        geometry.append(build_chunk(&points[start..end], style, start == 0));
        start = end - 1;
    }
    geometry.append(build_chunk(&points[start..], style, start == 0));
    geometry
}

fn build_chunk(points: &[Point], style: Style, start_cap: bool) -> Geometry {
    let options = options(style.width);
    let stroke_points = get_stroke_points(&inputs(points), &options);
    let mut outline = get_stroke_outline_points(&stroke_points, &options);
    if outline.first() == outline.last() {
        outline.pop();
    }
    if outline.len() < 3 {
        return Geometry::empty();
    }

    // This is the same midpoint-quadratic path Excalidraw sends to Path2D.
    let mut path = kurbo::BezPath::new();
    path.move_to((outline[0][0], outline[0][1]));
    for (index, &control) in outline.iter().enumerate() {
        let next = outline[(index + 1) % outline.len()];
        let midpoint = midpoint(control, next);
        path.quad_to((control[0], control[1]), (midpoint[0], midpoint[1]));
    }
    path.close_path();

    let mut geometry = Geometry::fill(path, FillRule::NonZero, style.color);
    if let (Some(start), Some(second), Some(penultimate), Some(end)) = (
        stroke_points.first(),
        stroke_points.get(1),
        stroke_points.get(stroke_points.len().saturating_sub(2)),
        stroke_points.last(),
    ) {
        let start = Point::new(start.point[0] as f32, start.point[1] as f32);
        let second = Point::new(second.point[0] as f32, second.point[1] as f32);
        let penultimate = Point::new(penultimate.point[0] as f32, penultimate.point[1] as f32);
        let end = Point::new(end.point[0] as f32, end.point[1] as f32);
        if start_cap {
            geometry.append(rounded_cap(
                start,
                start - second,
                style.width * 0.5,
                style.roundness,
                style.color,
            ));
        }
        geometry.append(rounded_cap(
            end,
            end - penultimate,
            style.width * 0.5,
            style.roundness,
            style.color,
        ));
    }
    geometry
}

fn inputs(points: &[Point]) -> Vec<InputPoint> {
    points
        .iter()
        .map(|point| InputPoint::Array([point.x as f64, point.y as f64], None))
        .collect()
}

fn options(width: f32) -> StrokeOptions {
    StrokeOptions {
        size: Some(width as f64),
        thinning: Some(0.0),
        smoothing: Some(0.5),
        streamline: Some(0.5),
        simulate_pressure: Some(false),
        start: Some(TaperOptions {
            cap: Some(false),
            ..TaperOptions::default()
        }),
        end: Some(TaperOptions {
            cap: Some(false),
            ..TaperOptions::default()
        }),
        last: Some(true),
        ..StrokeOptions::default()
    }
}

pub(super) fn rounded_cap(
    center: Point,
    outward: Point,
    radius: f32,
    roundness: f32,
    color: [f32; 4],
) -> Geometry {
    let length = outward.x.hypot(outward.y);
    if roundness <= 0.0 || radius <= 0.0 || length <= f32::EPSILON {
        return Geometry::empty();
    }
    let outward = Point::new(outward.x / length, outward.y / length);
    let normal = Point::new(-outward.y, outward.x);
    let mut path = kurbo::BezPath::new();
    let first = center + normal * radius;
    path.move_to((f64::from(first.x), f64::from(first.y)));
    for step in 1..=12 {
        let angle = std::f32::consts::FRAC_PI_2 - std::f32::consts::PI * step as f32 / 12.0;
        let point =
            center + outward * (angle.cos() * radius * roundness) + normal * (angle.sin() * radius);
        path.line_to((f64::from(point.x), f64::from(point.y)));
    }
    path.close_path();
    Geometry::fill(path, FillRule::NonZero, color)
}

fn midpoint(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
}
