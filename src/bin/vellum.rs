use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::SocketAddr;
use std::os::unix::net::UnixDatagram;
use std::time::Instant;

use clap::Parser;
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use wayland_client::backend::WaylandError;

mod cli;
mod config;
mod protocol;
mod render;
mod state;

use cli::{Cli, Command};
use config::Settings;
use protocol::CONTROL_SOCKET;

const MAX_SOCKET_MESSAGE: usize = 4096;
pub(crate) type Rgb = [f32; 3];

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
                    Command::Activate => state.set_input_active(true),
                    Command::Deactivate => state.set_input_active(false),
                    Command::Clear => state.clear(),
                    Command::ClearAndDeactivate => {
                        state.clear();
                        state.set_input_active(false);
                    }
                    Command::Exit => break 'running,
                }
            }
        }
        state.handle_timeouts(Instant::now());
    }
}
