use super::Modifiers;
use super::scene::{ElementKind, Point, Style, tessellate};
use crate::render::Geometry;

const SNAP_STEP: f32 = std::f32::consts::FRAC_PI_4;
const HIT_RADIUS: f32 = 9.0;
const VISUAL_RADIUS: f32 = 3.5;
const GAP: f32 = 4.0;
const COLOR: [f32; 4] = [0.1, 0.75, 1.0, 0.8];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Handle {
    Start,
    End,
    Corner(Corner),
    Edge(Edge),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum CursorHint {
    #[default]
    Crosshair,
    Move,
    NsResize,
    EwResize,
    NwseResize,
    NeswResize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Corner {
    TopLeft,
    TopRight,
    BottomRight,
    BottomLeft,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Edge {
    Top,
    Right,
    Bottom,
    Left,
}

pub(super) fn cursor(handle: Handle) -> CursorHint {
    match handle {
        Handle::Corner(corner) => resize_cursor(match corner {
            Corner::TopLeft => -3.0 * std::f32::consts::FRAC_PI_4,
            Corner::TopRight => -std::f32::consts::FRAC_PI_4,
            Corner::BottomRight => std::f32::consts::FRAC_PI_4,
            Corner::BottomLeft => 3.0 * std::f32::consts::FRAC_PI_4,
        }),
        Handle::Edge(Edge::Top | Edge::Bottom) => CursorHint::NsResize,
        Handle::Edge(Edge::Left | Edge::Right) => CursorHint::EwResize,
        Handle::Start | Handle::End => CursorHint::Move,
    }
}

fn resize_cursor(angle: f32) -> CursorHint {
    match ((angle / std::f32::consts::FRAC_PI_4).round() as i32).rem_euclid(4) {
        0 => CursorHint::EwResize,
        1 => CursorHint::NwseResize,
        2 => CursorHint::NsResize,
        _ => CursorHint::NeswResize,
    }
}

pub(super) fn hit_handle(kind: &ElementKind, point: Point) -> Option<Handle> {
    handle_points(kind)
        .into_iter()
        .filter(|(_, center)| center.distance_squared(point) <= HIT_RADIUS.powi(2))
        .min_by(|(_, first), (_, second)| {
            first
                .distance_squared(point)
                .total_cmp(&second.distance_squared(point))
        })
        .map(|(handle, _)| handle)
        .or_else(|| edge_handle(kind, point))
}

pub(super) fn outline(min: Point, max: Point) -> Geometry {
    tessellate(
        &ElementKind::Rectangle {
            min: Point::new(min.x - GAP, min.y - GAP),
            max: Point::new(max.x + GAP, max.y + GAP),
        },
        Style {
            width: 1.5,
            color: COLOR,
            roundness: 0.0,
        },
    )
}

pub(super) fn append_handles(kind: &ElementKind, output: &mut Vec<Geometry>) {
    output.extend(
        handle_points(kind)
            .into_iter()
            .map(|(handle, center)| handle_geometry(handle, center)),
    );
}

fn handle_points(kind: &ElementKind) -> Vec<(Handle, Point)> {
    if let ElementKind::Path {
        points,
        smooth: false,
        ..
    } = kind
        && points.len() >= 2
    {
        return vec![
            (Handle::Start, points[0]),
            (Handle::End, *points.last().expect("non-empty path")),
        ];
    }
    box_bounds(kind).map_or_else(Vec::new, |(min, max)| {
        vec![
            (Handle::Corner(Corner::TopLeft), min),
            (Handle::Corner(Corner::TopRight), Point::new(max.x, min.y)),
            (Handle::Corner(Corner::BottomRight), max),
            (Handle::Corner(Corner::BottomLeft), Point::new(min.x, max.y)),
        ]
    })
}

fn box_bounds(kind: &ElementKind) -> Option<(Point, Point)> {
    Some(match kind {
        ElementKind::Rectangle { min, max } => (*min, *max),
        ElementKind::Ellipse { center, radii } => (
            Point::new(center.x - radii.x, center.y - radii.y),
            Point::new(center.x + radii.x, center.y + radii.y),
        ),
        _ => return None,
    })
}

fn edge_handle(kind: &ElementKind, point: Point) -> Option<Handle> {
    let (min, max) = box_bounds(kind)?;
    [
        (Edge::Top, (point.y - min.y).abs(), point.x, min.x, max.x),
        (Edge::Right, (point.x - max.x).abs(), point.y, min.y, max.y),
        (Edge::Bottom, (point.y - max.y).abs(), point.x, min.x, max.x),
        (Edge::Left, (point.x - min.x).abs(), point.y, min.y, max.y),
    ]
    .into_iter()
    .filter(|(_, distance, along, start, end)| {
        *distance <= HIT_RADIUS && *along >= *start && *along <= *end
    })
    .min_by(|(_, first, ..), (_, second, ..)| first.total_cmp(second))
    .map(|(edge, ..)| Handle::Edge(edge))
}

fn handle_geometry(handle: Handle, center: Point) -> Geometry {
    let style = Style {
        width: 1.5,
        color: COLOR,
        roundness: 0.0,
    };
    match handle {
        Handle::Start | Handle::End => tessellate(
            &ElementKind::Ellipse {
                center,
                radii: Point::new(VISUAL_RADIUS, VISUAL_RADIUS),
            },
            style,
        ),
        Handle::Corner(corner) => {
            let inside = match corner {
                Corner::TopLeft => Point::new(1.0, 1.0),
                Corner::TopRight => Point::new(-1.0, 1.0),
                Corner::BottomRight => Point::new(-1.0, -1.0),
                Corner::BottomLeft => Point::new(1.0, -1.0),
            };
            let pivot = center - inside * GAP;
            tessellate(
                &ElementKind::Path {
                    points: vec![
                        Point::new(pivot.x + inside.x * 6.0, pivot.y),
                        pivot,
                        Point::new(pivot.x, pivot.y + inside.y * 6.0),
                    ],
                    smooth: false,
                    end_marker: None,
                },
                style,
            )
        }
        Handle::Edge(_) => Geometry::empty(),
    }
}

pub(super) fn resize(
    original: &ElementKind,
    handle: Handle,
    point: Point,
    modifiers: Modifiers,
) -> ElementKind {
    match (original, handle) {
        (
            ElementKind::Path {
                points,
                smooth: false,
                end_marker,
            },
            handle @ (Handle::Start | Handle::End),
        ) if points.len() >= 2 => {
            let mut points = points.clone();
            match handle {
                Handle::Start => {
                    points[0] = constrained_endpoint(
                        *points.last().expect("non-empty path"),
                        point,
                        modifiers.shift,
                    );
                }
                Handle::End => {
                    let start = points[0];
                    *points.last_mut().expect("non-empty path") =
                        constrained_endpoint(start, point, modifiers.shift);
                }
                _ => unreachable!(),
            }
            ElementKind::Path {
                points,
                smooth: false,
                end_marker: *end_marker,
            }
        }
        (ElementKind::Rectangle { min, max }, handle @ (Handle::Corner(_) | Handle::Edge(_))) => {
            let (min, max) = resized_box(*min, *max, handle, point, modifiers);
            ElementKind::Rectangle { min, max }
        }
        (
            ElementKind::Ellipse { center, radii },
            handle @ (Handle::Corner(_) | Handle::Edge(_)),
        ) => {
            let min = Point::new(center.x - radii.x, center.y - radii.y);
            let max = Point::new(center.x + radii.x, center.y + radii.y);
            let (min, max) = resized_box(min, max, handle, point, modifiers);
            ElementKind::Ellipse {
                center: min.midpoint(max),
                radii: Point::new((max.x - min.x) * 0.5, (max.y - min.y) * 0.5),
            }
        }
        _ => original.clone(),
    }
}

fn resized_box(
    original_min: Point,
    original_max: Point,
    handle: Handle,
    point: Point,
    modifiers: Modifiers,
) -> (Point, Point) {
    let center = original_min.midpoint(original_max);
    if let Handle::Corner(corner) = handle {
        let anchor = if modifiers.alt {
            center
        } else {
            opposite_corner(original_min, original_max, corner)
        };
        return constrained_box(anchor, point, modifiers.shift, modifiers.alt);
    }

    let Handle::Edge(edge) = handle else {
        return (original_min, original_max);
    };
    let (mut min, mut max) = (original_min, original_max);
    match (edge, modifiers.alt) {
        (Edge::Top | Edge::Bottom, true) => {
            let radius = (point.y - center.y).abs();
            min.y = center.y - radius;
            max.y = center.y + radius;
        }
        (Edge::Left | Edge::Right, true) => {
            let radius = (point.x - center.x).abs();
            min.x = center.x - radius;
            max.x = center.x + radius;
        }
        (Edge::Top, false) => (min.y, max.y) = ordered(point.y, original_max.y),
        (Edge::Right, false) => (min.x, max.x) = ordered(point.x, original_min.x),
        (Edge::Bottom, false) => (min.y, max.y) = ordered(point.y, original_min.y),
        (Edge::Left, false) => (min.x, max.x) = ordered(point.x, original_max.x),
    }
    if modifiers.shift {
        match edge {
            Edge::Top | Edge::Bottom => {
                let half = (max.y - min.y) * 0.5;
                min.x = center.x - half;
                max.x = center.x + half;
            }
            Edge::Left | Edge::Right => {
                let half = (max.x - min.x) * 0.5;
                min.y = center.y - half;
                max.y = center.y + half;
            }
        }
    }
    (min, max)
}

fn opposite_corner(min: Point, max: Point, corner: Corner) -> Point {
    match corner {
        Corner::TopLeft => max,
        Corner::TopRight => Point::new(min.x, max.y),
        Corner::BottomRight => min,
        Corner::BottomLeft => Point::new(max.x, min.y),
    }
}

fn ordered(first: f32, second: f32) -> (f32, f32) {
    (first.min(second), first.max(second))
}

pub(super) fn constrained_endpoint(start: Point, end: Point, snap: bool) -> Point {
    if !snap {
        return end;
    }
    let delta = end - start;
    let distance = delta.length();
    let angle = (delta.y.atan2(delta.x) / SNAP_STEP).round() * SNAP_STEP;
    Point::new(
        start.x + distance * angle.cos(),
        start.y + distance * angle.sin(),
    )
}

pub(super) fn constrained_box(
    start: Point,
    end: Point,
    square: bool,
    from_center: bool,
) -> (Point, Point) {
    let mut delta = end - start;
    if square {
        let size = delta.x.abs().max(delta.y.abs());
        delta.x = delta.x.signum() * size;
        delta.y = delta.y.signum() * size;
    }
    let end = start.translated(delta);
    if from_center {
        (
            Point::new(start.x - delta.x.abs(), start.y - delta.y.abs()),
            Point::new(start.x + delta.x.abs(), start.y + delta.y.abs()),
        )
    } else {
        (
            Point::new(start.x.min(end.x), start.y.min(end.y)),
            Point::new(start.x.max(end.x), start.y.max(end.y)),
        )
    }
}
