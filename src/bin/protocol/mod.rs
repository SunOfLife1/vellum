use crate::cli::Command;

pub const CONTROL_SOCKET: &str = "vellum.sock";

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
