#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Pen,
    Line,
    Arrow,
    Triangle,
    Rectangle,
    Ellipse,
    Text,
    Eraser,
    Select,
}

impl Tool {
    pub(super) fn supports_fill(self) -> bool {
        matches!(self, Self::Triangle | Self::Rectangle | Self::Ellipse)
    }

    pub(super) fn default_roundness(self) -> Option<f32> {
        match self {
            Self::Line | Self::Arrow => Some(0.5),
            Self::Triangle => Some(0.0),
            Self::Rectangle => Some(0.05),
            _ => None,
        }
    }
}

pub(super) const DEFAULT_ERASER_WIDTH: f32 = 10.0;
pub(super) const DEFAULT_TEXT_SIZE: f32 = 20.0;

#[derive(Clone, Copy)]
pub(super) struct ToolProperties {
    pub size: f32,
    pub opacity: f32,
    pub roundness: f32,
    pub filled: bool,
}

pub(super) struct ToolPropertySet {
    pen: ToolProperties,
    line: ToolProperties,
    arrow: ToolProperties,
    triangle: ToolProperties,
    rectangle: ToolProperties,
    ellipse: ToolProperties,
    text: ToolProperties,
    eraser: ToolProperties,
}

impl ToolPropertySet {
    pub(super) fn new(stroke_width: f32, default_fill_shapes: bool) -> Self {
        let properties = |size, roundness, filled| ToolProperties {
            size,
            opacity: 1.0,
            roundness,
            filled,
        };
        Self {
            pen: properties(stroke_width, 1.0, false),
            line: properties(stroke_width, 0.5, false),
            arrow: properties(stroke_width, 0.5, false),
            triangle: properties(stroke_width, 0.0, default_fill_shapes),
            rectangle: properties(stroke_width, 0.05, default_fill_shapes),
            ellipse: properties(stroke_width, 0.0, default_fill_shapes),
            text: properties(DEFAULT_TEXT_SIZE, 0.0, false),
            eraser: properties(DEFAULT_ERASER_WIDTH, 0.0, false),
        }
    }

    pub(super) fn properties(&self, tool: Tool) -> Option<&ToolProperties> {
        Some(match tool {
            Tool::Pen => &self.pen,
            Tool::Line => &self.line,
            Tool::Arrow => &self.arrow,
            Tool::Triangle => &self.triangle,
            Tool::Rectangle => &self.rectangle,
            Tool::Ellipse => &self.ellipse,
            Tool::Text => &self.text,
            Tool::Eraser => &self.eraser,
            Tool::Select => return None,
        })
    }

    pub(super) fn properties_mut(&mut self, tool: Tool) -> Option<&mut ToolProperties> {
        Some(match tool {
            Tool::Pen => &mut self.pen,
            Tool::Line => &mut self.line,
            Tool::Arrow => &mut self.arrow,
            Tool::Triangle => &mut self.triangle,
            Tool::Rectangle => &mut self.rectangle,
            Tool::Ellipse => &mut self.ellipse,
            Tool::Text => &mut self.text,
            Tool::Eraser => &mut self.eraser,
            Tool::Select => return None,
        })
    }
}

impl std::str::FromStr for Tool {
    type Err = &'static str;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        match name {
            "pen" => Ok(Self::Pen),
            "line" => Ok(Self::Line),
            "arrow" => Ok(Self::Arrow),
            "triangle" => Ok(Self::Triangle),
            "rectangle" => Ok(Self::Rectangle),
            "ellipse" => Ok(Self::Ellipse),
            "text" => Ok(Self::Text),
            "eraser" => Ok(Self::Eraser),
            "select" => Ok(Self::Select),
            _ => Err(
                "default tool must be pen, line, arrow, triangle, rectangle, ellipse, text, eraser, or select",
            ),
        }
    }
}
