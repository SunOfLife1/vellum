use super::Modifiers;
use super::scene::{Bounds, ElementKind, Point, Style, rendered_path_endpoints, tessellate};
use crate::render::{Geometry, Vertex};

const SNAP_STEP: f32 = std::f32::consts::FRAC_PI_4;
const ENDPOINT_HIT_RADIUS: f32 = 9.0;
const OUTLINE_HIT_RADIUS: f32 = 5.0;
const VISUAL_RADIUS: f32 = 4.5;
const SELECTION_WIDTH: f32 = 1.5;
const GAP: f32 = 4.0;
const COLOR: [f32; 4] = [0.1, 0.75, 1.0, 0.8];
const HANDLE_FILL: [f32; 4] = [0.04, 0.04, 0.04, 1.0];

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
        Handle::Corner(Corner::TopLeft | Corner::BottomRight) => CursorHint::NwseResize,
        Handle::Corner(Corner::TopRight | Corner::BottomLeft) => CursorHint::NeswResize,
        Handle::Edge(Edge::Top | Edge::Bottom) => CursorHint::NsResize,
        Handle::Edge(Edge::Left | Edge::Right) => CursorHint::EwResize,
        Handle::Start | Handle::End => CursorHint::Crosshair,
    }
}

pub(super) fn hit_handle(
    kind: &ElementKind,
    style: Style,
    bounds: Bounds,
    point: Point,
) -> Option<Handle> {
    rendered_path_endpoints(kind, style)
        .and_then(|[start, end]| {
            let radius_squared = ENDPOINT_HIT_RADIUS * ENDPOINT_HIT_RADIUS;
            let start_distance = start.distance_squared(point);
            let end_distance = end.distance_squared(point);
            let (handle, distance) = if start_distance < end_distance {
                (Handle::Start, start_distance)
            } else {
                (Handle::End, end_distance)
            };
            (distance <= radius_squared).then_some(handle)
        })
        .or_else(|| outline_handle(kind, bounds, point))
}

pub(super) fn outline(min: Point, max: Point) -> Geometry {
    tessellate(
        &ElementKind::Rectangle {
            min: Point::new(min.x - GAP, min.y - GAP),
            max: Point::new(max.x + GAP, max.y + GAP),
        },
        Style {
            width: SELECTION_WIDTH,
            color: COLOR,
            roundness: 0.0,
        },
    )
}

pub(super) fn append_handles(kind: &ElementKind, style: Style, output: &mut Vec<Geometry>) {
    if let Some([start, end]) = rendered_path_endpoints(kind, style) {
        let start_geometry = endpoint_geometry(start);
        let end_geometry = start_geometry.translated([end.x - start.x, end.y - start.y]);
        output.extend([start_geometry, end_geometry]);
    }
}

fn outline_handle(kind: &ElementKind, bounds: Bounds, point: Point) -> Option<Handle> {
    if !matches!(
        kind,
        ElementKind::Rectangle { .. } | ElementKind::Ellipse { .. }
    ) {
        return None;
    }
    let min = Point::new(bounds.min.x - GAP, bounds.min.y - GAP);
    let max = Point::new(bounds.max.x + GAP, bounds.max.y + GAP);
    if point.x < min.x - OUTLINE_HIT_RADIUS
        || point.x > max.x + OUTLINE_HIT_RADIUS
        || point.y < min.y - OUTLINE_HIT_RADIUS
        || point.y > max.y + OUTLINE_HIT_RADIUS
    {
        return None;
    }

    let left = (point.x - min.x).abs();
    let right = (point.x - max.x).abs();
    let x_edge = (left.min(right) <= OUTLINE_HIT_RADIUS).then_some(if left < right {
        Edge::Left
    } else {
        Edge::Right
    });
    let top = (point.y - min.y).abs();
    let bottom = (point.y - max.y).abs();
    let y_edge = (top.min(bottom) <= OUTLINE_HIT_RADIUS).then_some(if top < bottom {
        Edge::Top
    } else {
        Edge::Bottom
    });

    match (x_edge, y_edge) {
        (Some(Edge::Left), Some(Edge::Top)) => Some(Handle::Corner(Corner::TopLeft)),
        (Some(Edge::Right), Some(Edge::Top)) => Some(Handle::Corner(Corner::TopRight)),
        (Some(Edge::Right), Some(Edge::Bottom)) => Some(Handle::Corner(Corner::BottomRight)),
        (Some(Edge::Left), Some(Edge::Bottom)) => Some(Handle::Corner(Corner::BottomLeft)),
        (Some(edge), None) | (None, Some(edge)) => Some(Handle::Edge(edge)),
        _ => None,
    }
}

fn endpoint_geometry(center: Point) -> Geometry {
    const SEGMENTS: u32 = 16;
    let mut buffers = lyon_tessellation::VertexBuffers::new();
    buffers
        .vertices
        .push(Vertex::at([center.x, center.y], HANDLE_FILL));
    for index in 0..=SEGMENTS {
        let angle = std::f32::consts::TAU * index as f32 / SEGMENTS as f32;
        buffers.vertices.push(Vertex::at(
            [
                center.x + VISUAL_RADIUS * angle.cos(),
                center.y + VISUAL_RADIUS * angle.sin(),
            ],
            HANDLE_FILL,
        ));
    }
    for index in 0..SEGMENTS {
        buffers.indices.extend([0, index + 1, index + 2]);
    }
    let mut geometry = Geometry::new(buffers);
    let radius = VISUAL_RADIUS - SELECTION_WIDTH * 0.5;
    geometry.append(tessellate(
        &ElementKind::Ellipse {
            center,
            radii: Point::new(radius, radius),
        },
        Style {
            width: SELECTION_WIDTH,
            color: COLOR,
            roundness: 0.0,
        },
    ));
    geometry
}

pub(super) fn resize(
    original: &ElementKind,
    handle: Handle,
    delta: Point,
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
                        points[0] + delta,
                        modifiers.shift,
                    );
                }
                Handle::End => {
                    let start = points[0];
                    let end = *points.last().expect("non-empty path") + delta;
                    *points.last_mut().expect("non-empty path") =
                        constrained_endpoint(start, end, modifiers.shift);
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
            let (min, max) = resized_box(*min, *max, handle, delta, modifiers);
            ElementKind::Rectangle { min, max }
        }
        (
            ElementKind::Ellipse { center, radii },
            handle @ (Handle::Corner(_) | Handle::Edge(_)),
        ) => {
            let min = Point::new(center.x - radii.x, center.y - radii.y);
            let max = Point::new(center.x + radii.x, center.y + radii.y);
            let (min, max) = resized_box(min, max, handle, delta, modifiers);
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
    delta: Point,
    modifiers: Modifiers,
) -> (Point, Point) {
    let center = original_min.midpoint(original_max);
    let point = handle_position(original_min, original_max, handle) + delta;
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

fn handle_position(min: Point, max: Point, handle: Handle) -> Point {
    match handle {
        Handle::Corner(Corner::TopLeft) => min,
        Handle::Corner(Corner::TopRight) => Point::new(max.x, min.y),
        Handle::Corner(Corner::BottomRight) => max,
        Handle::Corner(Corner::BottomLeft) => Point::new(min.x, max.y),
        Handle::Edge(Edge::Top) => Point::new((min.x + max.x) * 0.5, min.y),
        Handle::Edge(Edge::Right) => Point::new(max.x, (min.y + max.y) * 0.5),
        Handle::Edge(Edge::Bottom) => Point::new((min.x + max.x) * 0.5, max.y),
        Handle::Edge(Edge::Left) => Point::new(min.x, (min.y + max.y) * 0.5),
        Handle::Start | Handle::End => unreachable!(),
    }
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
