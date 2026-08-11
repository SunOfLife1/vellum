#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Pen,
    Line,
    Arrow,
    Rectangle,
    Ellipse,
    Text,
    Eraser,
    Select,
}

impl Tool {
    pub(super) const PEN_ROUNDNESS: f32 = 1.0;
    pub(super) const LINE_ROUNDNESS: f32 = 0.5;
    pub(super) const ARROW_ROUNDNESS: f32 = 0.5;
    pub(super) const RECTANGLE_ROUNDNESS: f32 = 0.05;

    pub(super) fn properties(self) -> Option<(usize, Option<f32>)> {
        Some(match self {
            Self::Pen => (0, Some(Self::PEN_ROUNDNESS)),
            Self::Line => (1, Some(Self::LINE_ROUNDNESS)),
            Self::Arrow => (2, Some(Self::ARROW_ROUNDNESS)),
            Self::Rectangle => (3, Some(Self::RECTANGLE_ROUNDNESS)),
            Self::Ellipse => (4, None),
            Self::Text => (5, None),
            _ => return None,
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
            "rectangle" => Ok(Self::Rectangle),
            "ellipse" => Ok(Self::Ellipse),
            "text" => Ok(Self::Text),
            "eraser" => Ok(Self::Eraser),
            "select" => Ok(Self::Select),
            _ => Err(
                "default tool must be pen, line, arrow, rectangle, ellipse, text, eraser, or select",
            ),
        }
    }
}
