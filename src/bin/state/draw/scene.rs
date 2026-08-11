use super::freehand;
use crate::render::{Geometry, Vertex};

pub(super) const HIT_SLOP: f32 = 5.0;

pub type ElementId = u64;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub(super) fn distance_squared(self, other: Self) -> f32 {
        (self.x - other.x).powi(2) + (self.y - other.y).powi(2)
    }

    pub(super) fn length(self) -> f32 {
        self.x.hypot(self.y)
    }

    pub(super) fn midpoint(self, other: Self) -> Self {
        (self + other) * 0.5
    }

    pub(super) fn translated(self, delta: Self) -> Self {
        Self::new(self.x + delta.x, self.y + delta.y)
    }
}

impl std::ops::Sub for Point {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl std::ops::Add for Point {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl std::ops::Mul<f32> for Point {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Bounds {
    pub min: Point,
    pub max: Point,
}

impl Bounds {
    fn from_points(points: impl IntoIterator<Item = Point>) -> Self {
        let mut points = points.into_iter();
        let Some(first) = points.next() else {
            return Self::default();
        };
        let mut bounds = Self {
            min: first,
            max: first,
        };
        for point in points {
            bounds.min.x = bounds.min.x.min(point.x);
            bounds.min.y = bounds.min.y.min(point.y);
            bounds.max.x = bounds.max.x.max(point.x);
            bounds.max.y = bounds.max.y.max(point.y);
        }
        bounds
    }

    pub(super) fn expanded(self, amount: f32) -> Self {
        Self {
            min: Point::new(self.min.x - amount, self.min.y - amount),
            max: Point::new(self.max.x + amount, self.max.y + amount),
        }
    }

    pub fn contains(self, point: Point) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    pub width: f32,
    pub color: [f32; 4],
    pub roundness: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndMarker {
    Arrow,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ElementKind {
    Path {
        points: Vec<Point>,
        smooth: bool,
        end_marker: Option<EndMarker>,
    },
    Rectangle {
        min: Point,
        max: Point,
    },
    Ellipse {
        center: Point,
        radii: Point,
    },
    Text {
        origin: Point,
        content: String,
        font_size: f32,
    },
}

impl ElementKind {
    pub(super) fn translated(&self, delta: Point) -> Self {
        let mut translated = self.clone();
        match &mut translated {
            Self::Path { points, .. } => {
                points
                    .iter_mut()
                    .for_each(|point| *point = point.translated(delta));
            }
            Self::Rectangle { min, max } => {
                *min = min.translated(delta);
                *max = max.translated(delta);
            }
            Self::Ellipse { center, .. } => *center = center.translated(delta),
            Self::Text { origin, .. } => *origin = origin.translated(delta),
        }
        translated
    }
}

#[derive(Debug)]
pub struct Element {
    pub id: ElementId,
    pub kind: ElementKind,
    pub style: Style,
    pub bounds: Bounds,
    pub geometry: Geometry,
}

impl Element {
    pub(super) fn new(id: ElementId, kind: ElementKind, style: Style) -> Self {
        let geometry = tessellate(&kind, style);
        Self::with_geometry(id, kind, style, geometry)
    }

    pub(super) fn with_geometry(
        id: ElementId,
        kind: ElementKind,
        style: Style,
        geometry: Geometry,
    ) -> Self {
        let bounds = bounds_for(&kind, style.width);
        Self {
            id,
            kind,
            style,
            bounds,
            geometry,
        }
    }

    pub(super) fn replace(&mut self, kind: ElementKind, style: Style) -> (ElementKind, Style) {
        let kind = std::mem::replace(&mut self.kind, kind);
        let style = std::mem::replace(&mut self.style, style);
        self.bounds = bounds_for(&self.kind, self.style.width);
        self.geometry = tessellate(&self.kind, self.style);
        (kind, style)
    }

    pub(super) fn update_text_bounds(&mut self, [width, height]: [f32; 2]) {
        let ElementKind::Text { origin, .. } = self.kind else {
            return;
        };
        let half_width = self.style.width * 0.5;
        self.bounds = Bounds {
            min: origin,
            max: Point::new(origin.x + width, origin.y + height),
        }
        .expanded(half_width);
    }

    pub(super) fn preview_bounds(&self, kind: &ElementKind) -> Bounds {
        match (&self.kind, kind) {
            (
                ElementKind::Text { origin, .. },
                ElementKind::Text {
                    origin: preview, ..
                },
            ) => {
                let delta = *preview - *origin;
                Bounds {
                    min: self.bounds.min.translated(delta),
                    max: self.bounds.max.translated(delta),
                }
            }
            _ => bounds_for(kind, self.style.width),
        }
    }

    pub(super) fn hit_test(&self, point: Point) -> bool {
        if !self.bounds.expanded(HIT_SLOP).contains(point) {
            return false;
        }
        let tolerance = self.style.width * 0.5 + HIT_SLOP;
        match &self.kind {
            ElementKind::Path {
                points,
                smooth,
                end_marker,
            } => {
                let centerline = smooth.then(|| freehand::centerline(points, self.style.width));
                let points = centerline.as_deref().unwrap_or(points);
                polyline_hit(
                    points,
                    point,
                    if *smooth {
                        self.style.width * 0.8 + HIT_SLOP
                    } else {
                        tolerance
                    },
                ) || matches!(end_marker, Some(EndMarker::Arrow))
                    && path_endpoints(points).is_some_and(|(start, end)| {
                        let [tip, side_a, side_b] = arrow_head(start, end, self.style.width);
                        triangle_contains(point, tip, side_a, side_b)
                            || polyline_hit(&[tip, side_a, side_b, tip], point, tolerance)
                    })
            }
            ElementKind::Rectangle { min, max } => {
                rounded_rectangle_hit(*min, *max, self.style.roundness, point, tolerance)
            }
            ElementKind::Ellipse { center, radii } => {
                if radii.x <= f32::EPSILON || radii.y <= f32::EPSILON {
                    return point.distance_squared(*center) <= tolerance.powi(2);
                }
                let local = point - *center;
                let normalized = ((local.x / radii.x).powi(2) + (local.y / radii.y).powi(2)).sqrt();
                let normalized_tolerance = tolerance / radii.x.min(radii.y).max(1.0);
                (normalized - 1.0).abs() <= normalized_tolerance
            }
            ElementKind::Text { .. } => self.bounds.contains(point),
        }
    }
}

pub(super) fn bounds_for(kind: &ElementKind, width: f32) -> Bounds {
    let bounds = match kind {
        ElementKind::Path {
            points,
            end_marker: Some(EndMarker::Arrow),
            ..
        } => path_endpoints(points).map_or_else(
            || Bounds::from_points(points.iter().copied()),
            |(start, end)| {
                let head = arrow_head(start, end, width);
                Bounds::from_points(points.iter().copied().chain(head))
            },
        ),
        ElementKind::Path { points, .. } => Bounds::from_points(points.iter().copied()),
        ElementKind::Rectangle { min, max } => Bounds {
            min: *min,
            max: *max,
        },
        ElementKind::Ellipse { center, radii } => Bounds {
            min: Point::new(center.x - radii.x, center.y - radii.y),
            max: Point::new(center.x + radii.x, center.y + radii.y),
        },
        ElementKind::Text {
            origin,
            content,
            font_size,
        } => Bounds {
            min: *origin,
            max: Point::new(
                origin.x + content.chars().count().max(1) as f32 * font_size * 0.65,
                origin.y + font_size * 1.25,
            ),
        },
    };
    bounds.expanded(if matches!(kind, ElementKind::Path { smooth: true, .. }) {
        width * 0.8
    } else {
        width * 0.5
    })
}

pub(super) fn tessellate(kind: &ElementKind, style: Style) -> Geometry {
    use lyon_tessellation::path::Path;
    use lyon_tessellation::path::Winding;
    use lyon_tessellation::path::math::{Angle, point, vector};
    use lyon_tessellation::{
        BuffersBuilder, LineCap, LineJoin, StrokeOptions, StrokeTessellator, StrokeVertex,
    };

    if let ElementKind::Path {
        points,
        smooth: true,
        ..
    } = kind
    {
        return freehand::tessellate(points, style);
    }
    if let ElementKind::Rectangle { min, max } = kind {
        return tessellate_rectangle(*min, *max, style);
    }
    let marker = match kind {
        ElementKind::Path {
            points,
            end_marker: Some(EndMarker::Arrow),
            ..
        } => path_endpoints(points).map(|(start, end)| arrow_head(start, end, style.width)),
        _ => None,
    };
    let mut builder = Path::builder();
    let mut caps = Vec::new();
    match kind {
        ElementKind::Path {
            points,
            smooth: false,
            ..
        } => {
            if let Some(start) = points.first() {
                builder.begin(point(start.x, start.y));
                for (index, next) in points[1..].iter().enumerate() {
                    let next = if index + 2 == points.len() {
                        marker.map_or(*next, |[_, side_a, side_b]| side_a.midpoint(side_b))
                    } else {
                        *next
                    };
                    builder.line_to(point(next.x, next.y));
                }
                builder.end(false);
            }
            if let Some((start, end)) = path_endpoints(points) {
                caps.push((points[0], points[0] - points[1]));
                if marker.is_none() {
                    caps.push((end, end - start));
                }
            } else if let Some(point) = points.first() {
                caps.push((*point, Point::new(1.0, 0.0)));
            }
        }
        ElementKind::Path { smooth: true, .. } => unreachable!(),
        ElementKind::Rectangle { .. } => unreachable!(),
        ElementKind::Ellipse { center, radii } => {
            builder.add_ellipse(
                point(center.x, center.y),
                vector(radii.x, radii.y),
                Angle::zero(),
                Winding::Positive,
            );
        }
        ElementKind::Text { .. } => return Geometry::empty(),
    }

    let path = builder.build();
    let mut buffers = lyon_tessellation::VertexBuffers::new();
    StrokeTessellator::new()
        .tessellate_path(
            &path,
            &StrokeOptions::default()
                .with_line_width(style.width)
                .with_line_cap(LineCap::Butt)
                .with_line_join(LineJoin::Miter),
            &mut BuffersBuilder::new(&mut buffers, |vertex: StrokeVertex| {
                Vertex::at(vertex.position().to_array(), style.color)
            }),
        )
        .expect("valid annotation path");
    let mut geometry = Geometry::new(buffers);
    for (center, outward) in caps {
        geometry.append(freehand::rounded_cap(
            center,
            outward,
            style.width * 0.5,
            style.roundness,
            style.color,
        ));
    }
    if let Some(vertices) = marker {
        geometry.append(rounded_polygon(&vertices, style.roundness, style.color));
    }
    geometry
}

fn rectangle_radius(min: Point, max: Point, roundness: f32) -> f32 {
    ((max.x - min.x).abs().min((max.y - min.y).abs()) * 0.5) * roundness
}

fn tessellate_rectangle(min: Point, max: Point, style: Style) -> Geometry {
    use lyon_tessellation::{BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex};

    let half = style.width * 0.5;
    let maximum = (max.x - min.x).abs().min((max.y - min.y).abs()) * 0.5;
    let outer_radius = if style.roundness <= f32::EPSILON {
        0.0
    } else {
        half + maximum * style.roundness
    };
    let contours = [
        (
            Point::new(min.x - half, min.y - half),
            Point::new(max.x + half, max.y + half),
            outer_radius,
        ),
        (
            Point::new(min.x + half, min.y + half),
            Point::new(max.x - half, max.y - half),
            (maximum - half).max(0.0) * style.roundness,
        ),
    ];
    let mut builder = lyon_tessellation::path::Path::builder();
    for (min, max, radius) in contours {
        if min.x >= max.x || min.y >= max.y {
            continue;
        }
        let bounds = lyon_tessellation::math::Box2D::new(
            lyon_tessellation::math::point(min.x, min.y),
            lyon_tessellation::math::point(max.x, max.y),
        );
        if radius <= f32::EPSILON {
            builder.add_rectangle(&bounds, lyon_tessellation::path::Winding::Positive);
        } else {
            builder.add_rounded_rectangle(
                &bounds,
                &lyon_tessellation::path::builder::BorderRadii::new(radius),
                lyon_tessellation::path::Winding::Positive,
            );
        }
    }
    let mut buffers = lyon_tessellation::VertexBuffers::new();
    FillTessellator::new()
        .tessellate_path(
            &builder.build(),
            &FillOptions::default().with_fill_rule(FillRule::EvenOdd),
            &mut BuffersBuilder::new(&mut buffers, |vertex: FillVertex| {
                Vertex::at(vertex.position().to_array(), style.color)
            }),
        )
        .expect("valid rectangle");
    Geometry::new(buffers)
}

fn rounded_rectangle_hit(
    min: Point,
    max: Point,
    roundness: f32,
    point: Point,
    tolerance: f32,
) -> bool {
    let radius = rectangle_radius(min, max, roundness);
    let center = min.midpoint(max);
    let x = (point.x - center.x).abs() - ((max.x - min.x) * 0.5 - radius);
    let y = (point.y - center.y).abs() - ((max.y - min.y) * 0.5 - radius);
    let distance = x.max(0.0).hypot(y.max(0.0)) + x.max(y).min(0.0) - radius;
    distance.abs() <= tolerance
}

pub(super) fn default_roundness(kind: &ElementKind) -> Option<f32> {
    use super::tool::Tool;

    match kind {
        ElementKind::Path { smooth: true, .. } => Some(Tool::PEN_ROUNDNESS),
        ElementKind::Path {
            end_marker: Some(EndMarker::Arrow),
            ..
        } => Some(Tool::ARROW_ROUNDNESS),
        ElementKind::Path { .. } => Some(Tool::LINE_ROUNDNESS),
        ElementKind::Rectangle { .. } => Some(Tool::RECTANGLE_ROUNDNESS),
        _ => None,
    }
}

fn polyline_hit(points: &[Point], point: Point, tolerance: f32) -> bool {
    match points {
        [] => false,
        [only] => only.distance_squared(point) <= tolerance.powi(2),
        _ => points.windows(2).any(|segment| {
            segment_distance_squared(point, segment[0], segment[1]) <= tolerance.powi(2)
        }),
    }
}

fn arrow_head(start: Point, end: Point, width: f32) -> [Point; 3] {
    let delta = end - start;
    let length = delta.length();
    if length <= f32::EPSILON {
        return [end; 3];
    }
    let direction = Point::new(delta.x / length, delta.y / length);
    let normal = Point::new(-direction.y, direction.x);
    let size = (width * 5.0).clamp(16.0, 64.0).min(length * 0.8);
    let base = Point::new(end.x - direction.x * size, end.y - direction.y * size);
    let half = size * 0.45;
    [
        end,
        Point::new(base.x + normal.x * half, base.y + normal.y * half),
        Point::new(base.x - normal.x * half, base.y - normal.y * half),
    ]
}

fn rounded_polygon(vertices: &[Point], roundness: f32, color: [f32; 4]) -> Geometry {
    if vertices.len() < 3 {
        return Geometry::empty();
    }
    let mut outline = Vec::with_capacity(vertices.len() * 5);
    if roundness <= f32::EPSILON {
        outline.extend_from_slice(vertices);
    } else {
        let inset = 0.3 * roundness;
        for index in 0..vertices.len() {
            let vertex = vertices[index];
            let previous = vertices[(index + vertices.len() - 1) % vertices.len()];
            let next = vertices[(index + 1) % vertices.len()];
            let before = vertex + (previous - vertex) * inset;
            let after = vertex + (next - vertex) * inset;
            outline.push(before);
            for step in 1..=4 {
                let t = step as f32 * 0.25;
                let inverse = 1.0 - t;
                outline.push(
                    before * inverse.powi(2) + vertex * (2.0 * inverse * t) + after * t.powi(2),
                );
            }
        }
    }
    let center = outline
        .iter()
        .copied()
        .fold(Point::default(), |sum, point| sum + point)
        * (1.0 / outline.len() as f32);
    let mut buffers = lyon_tessellation::VertexBuffers {
        vertices: std::iter::once(center)
            .chain(outline.iter().copied())
            .map(|point| Vertex::at([point.x, point.y], color))
            .collect(),
        indices: Vec::with_capacity(outline.len() * 3),
    };
    for index in 0..outline.len() as u32 {
        buffers
            .indices
            .extend([0, index + 1, (index + 1) % outline.len() as u32 + 1]);
    }
    Geometry::new(buffers)
}

fn triangle_contains(point: Point, a: Point, b: Point, c: Point) -> bool {
    let side = |start: Point, end: Point| {
        (point.x - end.x) * (start.y - end.y) - (start.x - end.x) * (point.y - end.y)
    };
    let sides = [side(a, b), side(b, c), side(c, a)];
    !sides.iter().any(|side| *side < 0.0) || !sides.iter().any(|side| *side > 0.0)
}

fn path_endpoints(points: &[Point]) -> Option<(Point, Point)> {
    let end = *points.last()?;
    let start = *points.get(points.len().checked_sub(2)?)?;
    Some((start, end))
}

fn segment_distance_squared(point: Point, start: Point, end: Point) -> f32 {
    let delta = end - start;
    let length_squared = delta.distance_squared(Point::default());
    if length_squared <= f32::EPSILON {
        return point.distance_squared(start);
    }
    let offset = point - start;
    let fraction = ((offset.x * delta.x + offset.y * delta.y) / length_squared).clamp(0.0, 1.0);
    let projection = Point::new(start.x + delta.x * fraction, start.y + delta.y * fraction);
    point.distance_squared(projection)
}
