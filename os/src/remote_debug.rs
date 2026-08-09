//! Development-only TCP debug monitor.
//!
//! This is intentionally not SSH and provides no authentication or encryption.
//! It is compiled only with `remote-debug-monitor`, defaults to disabled, and
//! exposes diagnostic commands rather than a general-purpose process shell.
//! Keep it off production and untrusted networks. The QEMU launcher binds its
//! optional host forwarding to loopback for the same reason.
//!
//! TEST_STATUS: command transport is exercised on RISC-V QEMU/virtio-net. The
//! LoongArch runtime path and both physical-board NIC paths remain unverified.

extern crate alloc;

use alloc::format;
use core::str;

use network::{NetworkError, SocketRecvError, SocketRecvFinish, SocketRef, SocketSendError};
use runtime::logging::{info, warn};

const MONITOR_PORT : u16 = 2323;
const LISTEN_BACKLOG : usize = 1;
const MAX_LINE_LEN : usize = 128;
const RECEIVE_CHUNK : usize = 256;
const BANNER : &[u8] = b"WaterOS development monitor\r\nType 'help' for commands.\r\nwos> ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    Help,
    Ping,
    Status,
    Version,
    Quit,
    Empty,
    Unknown,
}

fn parse_command(line : &[u8]) -> Command {
    let Ok(text) = str::from_utf8(line) else {
        return Command::Unknown;
    };
    match text.trim() {
        "" => Command::Empty,
        "help" | "?" => Command::Help,
        "ping" => Command::Ping,
        "status" => Command::Status,
        "version" => Command::Version,
        "quit" | "exit" => Command::Quit,
        _ => Command::Unknown,
    }
}

fn send_all(socket : &SocketRef, mut data : &[u8]) -> Result<(), SocketSendError> {
    while !data.is_empty() {
        match socket.send(data) {
            Ok(0) | Err(SocketSendError::WouldBlock) => {
                let _ = task::sleep_for_ticks(1);
            }
            Ok(sent) => data = &data[sent..],
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn command_response(command : Command) -> (alloc::string::String, bool) {
    match command {
        Command::Help => {
            (alloc::string::String::from("commands: help, ping, status, version, quit\r\n"), false)
        }
        Command::Ping => (alloc::string::String::from("pong\r\n"), false),
        Command::Status => {
            let heap = runtime::heap_allocator::heap_mem_stats();
            let response = format!("tick={} online_cpus={:#x} heap_used={} heap_free={} \
                                    heap_capacity={}\r\n",
                                   task::current_tick(),
                                   task::online_cpu_mask().bits(),
                                   heap.used,
                                   heap.free,
                                   heap.capacity);
            (response, false)
        }
        Command::Version => (format!("WaterOS {}\r\n",
                                     env!("CARGO_PKG_VERSION")),
                             false),
        Command::Quit => (alloc::string::String::from("bye\r\n"), true),
        Command::Empty => (alloc::string::String::new(), false),
        Command::Unknown => {
            (alloc::string::String::from("unknown command; type 'help'\r\n"), false)
        }
    }
}

fn serve_client(socket : &SocketRef) {
    if send_all(socket, BANNER).is_err() {
        return;
    }
    let mut line = [0u8; MAX_LINE_LEN];
    let mut line_len = 0usize;
    let mut last_was_cr = false;
    loop {
        let lease = match socket.prepare_receive(RECEIVE_CHUNK) {
            Ok(lease) => lease,
            Err(SocketRecvError::Empty | SocketRecvError::Busy) => {
                task::sleep_for_ticks(1);
                continue;
            }
            Err(SocketRecvError::Finished) => return,
            Err(error) => {
                warn!("[remote-debug] receive failed: {:?}",
                      error);
                return;
            }
        };
        let received = lease.bytes().len();
        for &byte in lease.bytes() {
            if byte == b'\n' && last_was_cr {
                last_was_cr = false;
                continue;
            }
            if byte == b'\n' || byte == b'\r' {
                last_was_cr = byte == b'\r';
                let (response, close) = command_response(parse_command(&line[..line_len]));
                line_len = 0;
                if !response.is_empty() && send_all(socket, response.as_bytes()).is_err() {
                    return;
                }
                if close {
                    let _ = socket.shutdown();
                    return;
                }
                if send_all(socket, b"wos> ").is_err() {
                    return;
                }
            } else if byte == 0x08 || byte == 0x7F {
                last_was_cr = false;
                line_len = line_len.saturating_sub(1);
            } else if line_len < line.len() {
                last_was_cr = false;
                line[line_len] = byte;
                line_len += 1;
            }
        }
        if !matches!(lease.finish(received, true),
                     Ok(SocketRecvFinish::Bytes(_)))
        {
            return;
        }
    }
}

extern "C" fn monitor_task(_arg : usize) -> ! {
    let listener = loop {
        match SocketRef::new_tcp(0).and_then(|socket| {
                                       socket.bind(None, MONITOR_PORT)?;
                                       socket.listen(LISTEN_BACKLOG)?;
                                       Ok(socket)
                                   }) {
            Ok(socket) => break socket,
            Err(error) => {
                warn!("[remote-debug] listen on port {} failed: {}",
                      MONITOR_PORT, error);
                let _ = task::sleep_for_ticks(100);
            }
        }
    };
    info!("[remote-debug] unauthenticated development monitor listening on tcp/{}",
          MONITOR_PORT);
    loop {
        match listener.accept(0) {
            Ok((client, peer)) => {
                info!("[remote-debug] client connected from {:?}:{}",
                      peer.address, peer.port);
                serve_client(&client);
            }
            Err(NetworkError::NoPendingConnection) => {
                let _ = task::sleep_for_ticks(1);
            }
            Err(error) => {
                warn!("[remote-debug] accept failed: {}",
                      error);
                let _ = task::sleep_for_ticks(10);
            }
        }
    }
}

pub fn start() { task::spawn_kernel_task(monitor_task, 0); }

#[cfg(test)]
mod tests {
    use super::{parse_command, Command};

    #[test]
    fn command_parser_accepts_whitespace_and_aliases() {
        assert_eq!(parse_command(b" help \r\n"),
                   Command::Help);
        assert_eq!(parse_command(b"?"), Command::Help);
        assert_eq!(parse_command(b"exit"), Command::Quit);
        assert_eq!(parse_command(b"\t"), Command::Empty);
    }

    #[test]
    fn command_parser_rejects_invalid_or_unknown_input() {
        assert_eq!(parse_command(b"reboot"),
                   Command::Unknown);
        assert_eq!(parse_command(&[0xFF]), Command::Unknown);
    }
}
