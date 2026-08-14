use std::ffi::OsString;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::SocketAddr;
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use clap::Parser;
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use wayland_client::backend::WaylandError;

mod cli;
mod protocol;
mod render;
mod state;

use cli::{Backend, Cli, Command};
use protocol::CONTROL_SOCKET;

const MAX_SOCKET_MESSAGE: usize = 4096;
const CONFIG_FILE: &str = "vellum/config.toml";
pub(crate) type Rgb = [f32; 3];

const DEFAULT_PALETTE: [&str; 8] = [
    "#FF0000", "#FFFF00", "#00FF00", "#00FFFF", "#0000FF", "#FF00FF", "#FFFFFF", "#000000",
];

#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    default_tool: Option<String>,
    remember_last_tool: Option<bool>,
    stroke_width: Option<f32>,
    default_color: Option<String>,
    palette: Option<Vec<String>>,
    feedback_duration_ms: Option<u64>,
}

struct Settings {
    stroke_width: f32,
    stroke_color: Rgb,
    force_backend: Option<Backend>,
    default_tool: state::Tool,
    remember_last_tool: bool,
    palette: Vec<Rgb>,
    feedback_duration: Duration,
}

impl Settings {
    fn load(cli: Cli) -> Result<Self, String> {
        let file = if cli.no_config {
            FileConfig::default()
        } else if let Some(path) = &cli.config {
            read_config(path)?
        } else {
            read_first_config(default_config_paths())?
        };

        let stroke_width = file.stroke_width.unwrap_or(5.0);
        if !valid_width(stroke_width) {
            return Err("stroke_width must be a positive finite number".into());
        }

        let default_tool = file
            .default_tool
            .unwrap_or_else(|| "pen".into())
            .to_ascii_lowercase()
            .parse()?;

        let color_text = file.default_color.unwrap_or_else(|| "#FF0000".into());
        let stroke_color = parse_named_color("default_color", &color_text)?;

        let palette_text = file
            .palette
            .unwrap_or_else(|| DEFAULT_PALETTE.iter().map(ToString::to_string).collect());
        if !(2..=12).contains(&palette_text.len()) {
            return Err("palette must contain between 2 and 12 colors".into());
        }
        let palette = palette_text
            .iter()
            .enumerate()
            .map(|(index, color)| parse_named_color(&format!("palette[{index}]"), color))
            .collect::<Result<_, _>>()?;

        let feedback_duration_ms = file.feedback_duration_ms.unwrap_or(500);
        if feedback_duration_ms > 60_000 {
            return Err("feedback_duration_ms must not exceed 60000".into());
        }

        Ok(Self {
            stroke_width,
            stroke_color,
            force_backend: cli.force_backend,
            default_tool,
            remember_last_tool: file.remember_last_tool.unwrap_or(true),
            palette,
            feedback_duration: Duration::from_millis(feedback_duration_ms),
        })
    }
}

fn parse_named_color(name: &str, value: &str) -> Result<Rgb, String> {
    parse_color(value).map_err(|error| format!("invalid {name} {value:?}: {error}"))
}

fn valid_width(width: f32) -> bool {
    width.is_finite() && width > 0.0
}

fn parse_color(value: &str) -> Result<Rgb, &'static str> {
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

fn read_config(path: &Path) -> Result<FileConfig, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    parse_config(path, &contents)
}

fn read_optional_config(path: &Path) -> Result<Option<FileConfig>, String> {
    match std::fs::read_to_string(path) {
        Ok(contents) => parse_config(path, &contents).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("could not read {}: {error}", path.display())),
    }
}

fn parse_config(path: &Path, contents: &str) -> Result<FileConfig, String> {
    toml::from_str(contents).map_err(|error| format!("invalid {}: {error}", path.display()))
}

fn read_first_config(paths: impl IntoIterator<Item = PathBuf>) -> Result<FileConfig, String> {
    for path in paths {
        if let Some(config) = read_optional_config(&path)? {
            return Ok(config);
        }
    }
    Ok(FileConfig::default())
}

fn default_config_paths() -> Vec<PathBuf> {
    config_paths(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
        std::env::var_os("XDG_CONFIG_DIRS"),
    )
}

fn config_paths(
    xdg_config_home: Option<OsString>,
    home: Option<OsString>,
    xdg_config_dirs: Option<OsString>,
) -> Vec<PathBuf> {
    let user = absolute_path(xdg_config_home)
        .or_else(|| absolute_path(home).map(|path| path.join(".config")));
    let mut paths: Vec<_> = user
        .into_iter()
        .map(|path| path.join(CONFIG_FILE))
        .collect();

    match xdg_config_dirs.filter(|value| !value.is_empty()) {
        Some(dirs) => paths.extend(
            std::env::split_paths(&dirs)
                .filter(|path| path.is_absolute())
                .map(|path| path.join(CONFIG_FILE)),
        ),
        None => paths.push(PathBuf::from("/etc/xdg").join(CONFIG_FILE)),
    }
    paths
}

fn absolute_path(value: Option<OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|path| path.is_absolute())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("vellum: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let arguments = Cli::parse();
    if let Some(subcommand) = &arguments.command {
        return send_command(subcommand);
    }
    let settings = Settings::load(arguments)?;
    run_overlay(settings);
    Ok(())
}

fn send_command(command: &Command) -> Result<(), String> {
    let socket_addr =
        SocketAddr::from_abstract_name(CONTROL_SOCKET).map_err(|error| error.to_string())?;
    let socket = UnixDatagram::unbound().map_err(|error| error.to_string())?;
    socket
        .connect_addr(&socket_addr)
        .map_err(|error| format!("could not connect to the overlay: {error}"))?;
    socket
        .send(command.serialize())
        .map_err(|error| format!("could not send command: {error}"))?;
    Ok(())
}

fn run_overlay(settings: Settings) {
    // setup socket for messages
    let socket_addr = SocketAddr::from_abstract_name(CONTROL_SOCKET).unwrap();
    let socket = match UnixDatagram::bind_addr(&socket_addr) {
        Ok(socket) => socket,
        Err(error) => {
            eprintln!("vellum: could not bind control socket: {error}");
            std::process::exit(1);
        }
    };
    socket.set_nonblocking(true).unwrap_or_else(|error| {
        eprintln!("vellum: could not configure control socket: {error}");
        std::process::exit(1);
    });

    let (mut state, mut event_queue) =
        state::State::setup_wayland(settings).unwrap_or_else(|error| {
            eprintln!("vellum: {error}");
            std::process::exit(1);
        });
    state.deactivate();

    'running: loop {
        if let Err(error) = event_queue.dispatch_pending(&mut state) {
            eprintln!("vellum: Wayland dispatch failed: {error}");
            break;
        }
        let flush_blocked = match event_queue.flush() {
            Ok(()) => false,
            Err(WaylandError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => true,
            Err(error) => {
                eprintln!("vellum: Wayland flush failed: {error}");
                break;
            }
        };

        let Some(read_guard) = event_queue.prepare_read() else {
            continue;
        };
        let timeout = state.next_wakeup().map(|deadline| {
            let remaining = deadline.saturating_duration_since(Instant::now());
            Timespec {
                tv_sec: remaining.as_secs() as _,
                tv_nsec: remaining.subsec_nanos() as _,
            }
        });
        let (wayland_ready, socket_ready) = {
            let mut fds = [
                PollFd::new(
                    &event_queue,
                    PollFlags::IN
                        | if flush_blocked {
                            PollFlags::OUT
                        } else {
                            PollFlags::empty()
                        },
                ),
                PollFd::new(&socket, PollFlags::IN),
            ];
            if let Err(error) = poll(&mut fds, timeout.as_ref()) {
                if error == rustix::io::Errno::INTR {
                    continue;
                }
                eprintln!("vellum: event polling failed: {error}");
                break 'running;
            }
            (
                fds[0].revents().contains(PollFlags::IN),
                fds[1].revents().contains(PollFlags::IN),
            )
        };
        if wayland_ready {
            if let Err(error) = read_guard.read() {
                eprintln!("vellum: Wayland read failed: {error}");
                break;
            }
        } else {
            drop(read_guard);
        }

        if socket_ready {
            let mut message = [0; MAX_SOCKET_MESSAGE + 1];
            loop {
                let size = match socket.recv(&mut message) {
                    Ok(size) => size,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(error) => {
                        eprintln!("vellum: socket read failed: {error}");
                        break 'running;
                    }
                };
                if size > MAX_SOCKET_MESSAGE {
                    eprintln!("vellum: socket message exceeded {MAX_SOCKET_MESSAGE} bytes");
                    continue;
                }
                let command = match Command::deserialize(&message[..size]) {
                    Ok(command) => command,
                    Err(error) => {
                        eprintln!("{error}");
                        continue;
                    }
                };
                match command {
                    Command::Toggle => state.toggle_input(),
                    Command::Exit => break 'running,
                }
            }
        }
        state.handle_timeouts(Instant::now());
    }
}
