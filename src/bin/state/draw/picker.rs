use super::scene::Point;
use super::tool::Tool;
use crate::render::{Geometry, Vertex};

const INNER_RADIUS: f32 = 30.0;
const OUTER_RADIUS: f32 = 88.0;
const WHEEL_BORDER_WIDTH: f32 = 3.0;
const HOVER_EXTENSION: f32 = 6.0;
const PREVIEW_BORDER_WIDTH: f32 = 1.0;
const SEPARATOR_HALF_WIDTH: f32 = 2.0;

const GAP_LINE_COLOR: [f32; 4] = [0.03, 0.03, 0.03, 1.0];

#[derive(Debug, Clone, Copy)]
pub(super) enum Picker {
    Color {
        center: Point,
        hovered: Option<usize>,
    },
    Tool {
        center: Point,
        hovered: Option<Tool>,
    },
}

pub(super) fn palette_choice(center: Point, point: Point, color_count: usize) -> Option<usize> {
    radial_index(center, point, color_count)
}

const TOOL_CHOICES: [Tool; 8] = [
    Tool::Pen,
    Tool::Line,
    Tool::Arrow,
    Tool::Rectangle,
    Tool::Ellipse,
    Tool::Text,
    Tool::Select,
    Tool::Eraser,
];

pub(super) fn tool_choice(center: Point, point: Point) -> Option<Tool> {
    radial_index(center, point, TOOL_CHOICES.len()).map(|index| TOOL_CHOICES[index])
}

fn radial_index(center: Point, point: Point, count: usize) -> Option<usize> {
    let delta = point - center;
    if delta.length() < INNER_RADIUS {
        return None;
    }
    let step = std::f32::consts::TAU / count as f32;
    Some(
        ((delta.y.atan2(delta.x) + step * 0.5).rem_euclid(std::f32::consts::TAU) / step).floor()
            as usize,
    )
}

fn radial_point(center: Point, radius: f32, angle: f32) -> [f32; 2] {
    [
        center.x + radius * angle.cos(),
        center.y + radius * angle.sin(),
    ]
}

fn push_disc(
    buffers: &mut lyon_tessellation::VertexBuffers<Vertex, u32>,
    center: Point,
    radius: f32,
    color: [f32; 4],
) {
    const SEGMENTS: usize = 48;
    let base = buffers.vertices.len() as u32;
    buffers
        .vertices
        .push(Vertex::at([center.x, center.y], color));
    for index in 0..=SEGMENTS {
        let angle = std::f32::consts::TAU * index as f32 / SEGMENTS as f32;
        buffers
            .vertices
            .push(Vertex::at(radial_point(center, radius, angle), color));
    }
    for index in 0..SEGMENTS as u32 {
        buffers
            .indices
            .extend([base, base + index + 1, base + index + 2]);
    }
}

fn push_color_preview(
    buffers: &mut lyon_tessellation::VertexBuffers<Vertex, u32>,
    center: Point,
    color: [f32; 4],
) {
    let radius = INNER_RADIUS - WHEEL_BORDER_WIDTH;
    push_disc(buffers, center, radius, GAP_LINE_COLOR);
    push_disc(
        buffers,
        center,
        radius - PREVIEW_BORDER_WIDTH,
        opaque(color),
    );
}

fn push_wedge(
    buffers: &mut lyon_tessellation::VertexBuffers<Vertex, u32>,
    center: Point,
    inner: f32,
    outer: f32,
    angles: std::ops::Range<f32>,
    edge_inset: f32,
    color: [f32; 4],
) {
    const SEGMENTS: usize = 8;
    let base = buffers.vertices.len() as u32;
    let inner_inset = (edge_inset / inner).asin();
    let outer_inset = (edge_inset / outer).asin();
    let span = angles.end - angles.start;
    for index in 0..=SEGMENTS {
        let fraction = index as f32 / SEGMENTS as f32;
        let inner_angle = angles.start + inner_inset + (span - 2.0 * inner_inset) * fraction;
        let outer_angle = angles.start + outer_inset + (span - 2.0 * outer_inset) * fraction;
        buffers.vertices.extend([
            Vertex::at(radial_point(center, inner, inner_angle), color),
            Vertex::at(radial_point(center, outer, outer_angle), color),
        ]);
    }
    for index in 0..SEGMENTS as u32 {
        let first = base + index * 2;
        buffers
            .indices
            .extend([first, first + 1, first + 3, first, first + 3, first + 2]);
    }
}

pub(super) fn palette_geometry(
    center: Point,
    hovered: Option<usize>,
    current_color: [f32; 4],
    palette: &[[f32; 3]],
) -> Geometry {
    let mut buffers = lyon_tessellation::VertexBuffers::new();
    push_color_preview(&mut buffers, center, current_color);
    let step = std::f32::consts::TAU / palette.len() as f32;
    for (index, &[red, green, blue]) in palette.iter().enumerate() {
        let selected = hovered == Some(index);
        let outer = OUTER_RADIUS + if selected { HOVER_EXTENSION } else { 0.0 };
        let start = index as f32 * step - step * 0.5;
        let end = start + step;
        push_wedge(
            &mut buffers,
            center,
            INNER_RADIUS - WHEEL_BORDER_WIDTH,
            outer + WHEEL_BORDER_WIDTH,
            start..end,
            0.0,
            GAP_LINE_COLOR,
        );
        push_wedge(
            &mut buffers,
            center,
            INNER_RADIUS,
            outer,
            start..end,
            SEPARATOR_HALF_WIDTH,
            [red, green, blue, 0.98],
        );
    }
    Geometry::new(buffers)
}

pub(super) fn tool_palette_geometry(
    center: Point,
    hovered: Option<Tool>,
    active: Tool,
    current_color: [f32; 4],
) -> Geometry {
    let mut buffers = lyon_tessellation::VertexBuffers::new();
    push_color_preview(&mut buffers, center, current_color);
    let step = std::f32::consts::TAU / TOOL_CHOICES.len() as f32;
    for (index, tool) in TOOL_CHOICES.iter().copied().enumerate() {
        let is_hovered = hovered == Some(tool);
        let is_active = tool == active;
        let outer = OUTER_RADIUS + if is_hovered { HOVER_EXTENSION } else { 0.0 };
        let start = index as f32 * step - step * 0.5;
        let end = start + step;
        push_wedge(
            &mut buffers,
            center,
            INNER_RADIUS - WHEEL_BORDER_WIDTH,
            outer + WHEEL_BORDER_WIDTH,
            start..end,
            0.0,
            GAP_LINE_COLOR,
        );
        push_wedge(
            &mut buffers,
            center,
            INNER_RADIUS,
            outer,
            start..end,
            SEPARATOR_HALF_WIDTH,
            if is_hovered {
                [0.1, 0.75, 1.0, 0.98]
            } else if is_active {
                [0.12, 0.38, 0.5, 0.97]
            } else {
                [0.16, 0.18, 0.22, 0.97]
            },
        );
        let icon_center = Point::new(
            center.x + 60.0 * (index as f32 * step).cos(),
            center.y + 60.0 * (index as f32 * step).sin(),
        );
        push_tool_icon(&mut buffers, icon_center, tool);
    }
    Geometry::new(buffers)
}

fn opaque([red, green, blue, _]: [f32; 4]) -> [f32; 4] {
    [red, green, blue, 1.0]
}

fn push_tool_icon(
    buffers: &mut lyon_tessellation::VertexBuffers<Vertex, u32>,
    center: Point,
    tool: Tool,
) {
    use lyon_tessellation::path::Path;
    use lyon_tessellation::path::math::point;
    use lyon_tessellation::{
        BuffersBuilder, LineCap, LineJoin, StrokeOptions, StrokeTessellator, StrokeVertex,
    };

    // Adapted from Lucide's 24px outline icons; see the third-party notice in LICENSE.
    const SCALE: f32 = 0.82;
    let p = |x: f32, y: f32| point(center.x + (x - 12.0) * SCALE, center.y + (y - 12.0) * SCALE);
    let mut builder = Path::builder();
    match tool {
        Tool::Pen => {
            builder.begin(p(13.0, 21.0));
            builder.line_to(p(21.0, 21.0));
            builder.end(false);
            builder.begin(p(3.8, 16.2));
            builder.line_to(p(17.2, 2.8));
            builder.line_to(p(21.2, 6.8));
            builder.line_to(p(7.8, 20.2));
            builder.line_to(p(2.5, 21.5));
            builder.close();
        }
        Tool::Line => {
            builder.begin(p(5.0, 12.0));
            builder.line_to(p(19.0, 12.0));
            builder.end(false);
        }
        Tool::Arrow => {
            builder.begin(p(7.0, 7.0));
            builder.line_to(p(17.0, 7.0));
            builder.line_to(p(17.0, 17.0));
            builder.end(false);
            builder.begin(p(7.0, 17.0));
            builder.line_to(p(17.0, 7.0));
            builder.end(false);
        }
        Tool::Rectangle => {
            builder.begin(p(5.0, 3.0));
            builder.line_to(p(19.0, 3.0));
            builder.line_to(p(21.0, 5.0));
            builder.line_to(p(21.0, 19.0));
            builder.line_to(p(19.0, 21.0));
            builder.line_to(p(5.0, 21.0));
            builder.line_to(p(3.0, 19.0));
            builder.line_to(p(3.0, 5.0));
            builder.close();
        }
        Tool::Ellipse => {
            builder.begin(p(22.0, 12.0));
            for index in 1..=32 {
                let angle = std::f32::consts::TAU * index as f32 / 32.0;
                builder.line_to(p(12.0 + 10.0 * angle.cos(), 12.0 + 10.0 * angle.sin()));
            }
            builder.close();
        }
        Tool::Text => {
            builder.begin(p(12.0, 4.0));
            builder.line_to(p(12.0, 20.0));
            builder.end(false);
            builder.begin(p(4.0, 7.0));
            builder.line_to(p(4.0, 4.0));
            builder.line_to(p(20.0, 4.0));
            builder.line_to(p(20.0, 7.0));
            builder.end(false);
            builder.begin(p(9.0, 20.0));
            builder.line_to(p(15.0, 20.0));
            builder.end(false);
        }
        Tool::Select => {
            builder.begin(p(4.0, 4.7));
            builder.line_to(p(20.7, 11.0));
            builder.line_to(p(14.5, 13.0));
            builder.line_to(p(12.0, 20.7));
            builder.close();
        }
        Tool::Eraser => {
            builder.begin(p(5.1, 11.1));
            builder.line_to(p(12.6, 3.6));
            builder.line_to(p(21.4, 12.4));
            builder.line_to(p(12.8, 21.0));
            builder.line_to(p(8.0, 21.0));
            builder.line_to(p(2.6, 15.6));
            builder.close();
            builder.begin(p(5.1, 11.1));
            builder.line_to(p(13.9, 19.9));
            builder.end(false);
        }
    }
    let path = builder.build();
    StrokeTessellator::new()
        .tessellate_path(
            &path,
            &StrokeOptions::default()
                .with_line_width(2.0)
                .with_line_cap(LineCap::Round)
                .with_line_join(LineJoin::Round),
            &mut BuffersBuilder::new(buffers, |vertex: StrokeVertex| {
                Vertex::at(vertex.position().to_array(), [0.96, 0.97, 1.0, 1.0])
            }),
        )
        .expect("valid tool icon");
}
