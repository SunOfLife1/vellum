use kurbo::{Affine, BezPath, Cap, Join, Stroke};

#[derive(Debug, Clone, Copy)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

#[derive(Debug, Clone)]
pub struct StrokeStyle {
    pub width: f64,
    pub join: Join,
    pub start_cap: Cap,
    pub end_cap: Cap,
    pub miter_limit: f64,
}

impl StrokeStyle {
    pub fn new(width: f64) -> Self {
        Self {
            width,
            join: Join::Miter,
            start_cap: Cap::Butt,
            end_cap: Cap::Butt,
            miter_limit: 4.0,
        }
    }

    pub fn round(width: f64) -> Self {
        Self {
            width,
            join: Join::Round,
            start_cap: Cap::Round,
            end_cap: Cap::Round,
            miter_limit: 4.0,
        }
    }

    pub(super) fn as_kurbo(&self) -> Stroke {
        Stroke {
            width: self.width,
            join: self.join,
            miter_limit: self.miter_limit,
            start_cap: self.start_cap,
            end_cap: self.end_cap,
            ..Stroke::default()
        }
    }
}

#[derive(Debug, Clone)]
pub enum DrawCommand {
    Fill {
        path: BezPath,
        fill_rule: FillRule,
        color: [f32; 4],
    },
    Stroke {
        path: BezPath,
        stroke: StrokeStyle,
        color: [f32; 4],
    },
}

#[derive(Debug, Clone, Default)]
pub struct Geometry {
    pub(super) commands: Vec<DrawCommand>,
}

pub struct LocalGeometry {
    pub(super) geometry: Geometry,
    pub(super) origin: [f32; 2],
    pub(super) size: [u32; 2],
}

impl LocalGeometry {
    pub fn new(geometry: Geometry, origin: [f32; 2], size: [u32; 2]) -> Self {
        Self {
            geometry,
            origin,
            size,
        }
    }
}

impl Geometry {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn fill(path: BezPath, fill_rule: FillRule, color: [f32; 4]) -> Self {
        Self {
            commands: vec![DrawCommand::Fill {
                path,
                fill_rule,
                color,
            }],
        }
    }

    pub fn stroke(path: BezPath, stroke: StrokeStyle, color: [f32; 4]) -> Self {
        Self {
            commands: vec![DrawCommand::Stroke {
                path,
                stroke,
                color,
            }],
        }
    }

    pub fn append(&mut self, other: Self) {
        self.commands.extend(other.commands);
    }

    pub fn translated(&self, offset: [f32; 2]) -> Self {
        let transform = Affine::translate((f64::from(offset[0]), f64::from(offset[1])));
        Self {
            commands: self
                .commands
                .iter()
                .map(|command| match command {
                    DrawCommand::Fill {
                        path,
                        fill_rule,
                        color,
                    } => DrawCommand::Fill {
                        path: transform * path,
                        fill_rule: *fill_rule,
                        color: *color,
                    },
                    DrawCommand::Stroke {
                        path,
                        stroke,
                        color,
                    } => DrawCommand::Stroke {
                        path: transform * path,
                        stroke: stroke.clone(),
                        color: *color,
                    },
                })
                .collect(),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}
