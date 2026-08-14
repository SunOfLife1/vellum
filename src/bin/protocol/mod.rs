pub type Rgb = [f32; 3];

pub const CONTROL_SOCKET: &str = "vellum.sock";

#[derive(Debug, PartialEq, clap::Subcommand)]
pub enum Command {
    /// Activate or deactivate drawing mode
    Toggle,
    /// Exit the running overlay
    Exit,
}

impl Command {
    pub fn serialize(&self) -> &'static [u8] {
        match self {
            Self::Toggle => b"toggle",
            Self::Exit => b"exit",
        }
    }

    pub fn deserialize(message: &[u8]) -> Result<Self, &'static str> {
        match message {
            b"toggle" => Ok(Self::Toggle),
            b"exit" => Ok(Self::Exit),
            _ => Err("invalid command"),
        }
    }
}

pub fn valid_width(width: f32) -> bool {
    width.is_finite() && width > 0.0
}

pub fn parse_color(value: &str) -> Result<Rgb, &'static str> {
    let hex = value.strip_prefix('#').ok_or("color must start with #")?;
    if hex.len() != 6 {
        return Err("color must be #RRGGBB");
    }
    let value = u32::from_str_radix(hex, 16).map_err(|_| "color contains a non-hex digit")?;
    Ok([
        ((value >> 16) & 0xff) as f32 / 255.0,
        ((value >> 8) & 0xff) as f32 / 255.0,
        (value & 0xff) as f32 / 255.0,
    ])
}
