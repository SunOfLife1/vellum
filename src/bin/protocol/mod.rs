pub type Color = [f32; 4];

pub const CONTROL_SOCKET: &str = "vellum.sock";

#[derive(Debug, PartialEq, clap::Subcommand)]
pub enum Command {
    /// Toggle drawing mode
    Toggle,
    /// Undo the last stroke
    Undo,
    /// Redo the last undone stroke
    Redo,
    /// Clear the canvas
    Clear,
    /// Clear and deactivate
    ClearAndDeactivate,
    /// Set stroke width in pixels
    StrokeWidth {
        #[arg(value_parser = parse_width)]
        width: f32,
    },
    /// Set stroke color (hex, optional alpha)
    StrokeColor {
        #[arg(value_parser = parse_color)]
        color: Color,
    },
    /// Exit the app
    Exit,
}

impl Command {
    pub fn serialize(&self) -> String {
        match self {
            Self::Toggle => "toggle".into(),
            Self::Undo => "undo".into(),
            Self::Redo => "redo".into(),
            Self::Clear => "clear".into(),
            Self::ClearAndDeactivate => "clear_and_deactivate".into(),
            Self::StrokeWidth { width } => format!("stroke_width {width}"),
            Self::StrokeColor { color } => format!("stroke_color {}", format_color(*color)),
            Self::Exit => "exit".into(),
        }
    }

    pub fn deserialize(message: &[u8]) -> Result<Self, &'static str> {
        let message = std::str::from_utf8(message).map_err(|_| "message is not UTF-8")?;
        let mut parts = message.split_whitespace();
        let command = match parts.next() {
            Some("toggle") => Self::Toggle,
            Some("undo") => Self::Undo,
            Some("redo") => Self::Redo,
            Some("clear") => Self::Clear,
            Some("clear_and_deactivate") => Self::ClearAndDeactivate,
            Some("stroke_width") => {
                let width = parse_width(parts.next().ok_or("stroke_width requires a value")?)?;
                Self::StrokeWidth { width }
            }
            Some("stroke_color") => Self::StrokeColor {
                color: parse_color(parts.next().ok_or("stroke_color requires a value")?)?,
            },
            Some("exit") => Self::Exit,
            Some(_) => return Err("unknown command"),
            None => return Err("empty command"),
        };
        if parts.next().is_some() {
            return Err("unexpected command arguments");
        }
        Ok(command)
    }
}

pub fn valid_width(width: f32) -> bool {
    width.is_finite() && width > 0.0
}

fn parse_width(value: &str) -> Result<f32, &'static str> {
    let width = value.parse().map_err(|_| "invalid stroke width")?;
    if !valid_width(width) {
        return Err("stroke width must be positive and finite");
    }
    Ok(width)
}

pub fn parse_color(value: &str) -> Result<Color, &'static str> {
    let hex = value.strip_prefix('#').ok_or("color must start with #")?;
    if !matches!(hex.len(), 6 | 8) {
        return Err("color must be #RRGGBB or #RRGGBBAA");
    }
    let value = u32::from_str_radix(hex, 16).map_err(|_| "color contains a non-hex digit")?;
    let value = if hex.len() == 6 {
        (value << 8) | 0xff
    } else {
        value
    };
    Ok([
        ((value >> 24) & 0xff) as f32 / 255.0,
        ((value >> 16) & 0xff) as f32 / 255.0,
        ((value >> 8) & 0xff) as f32 / 255.0,
        (value & 0xff) as f32 / 255.0,
    ])
}

fn format_color(color: Color) -> String {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    let [red, green, blue, alpha] = color.map(channel);
    if alpha == 255 {
        format!("#{red:02X}{green:02X}{blue:02X}")
    } else {
        format!("#{red:02X}{green:02X}{blue:02X}{alpha:02X}")
    }
}
