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
    Ls2kIrq,
    Ls2kMmc,
    Capabilities,
    Devfs,
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
        "ls2k-irq" => Command::Ls2kIrq,
        "ls2k-mmc" => Command::Ls2kMmc,
        "capabilities" | "caps" => Command::Capabilities,
        "devfs" => Command::Devfs,
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
        Command::Help => (alloc::string::String::from("commands: help, ping, status, version, \
                                                       capabilities, devfs, ls2k-irq, ls2k-mmc, quit\r\n"),
                          false),
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
        Command::Ls2kIrq => (ls2k_irq_response(), false),
        Command::Ls2kMmc => (ls2k_mmc_response(), false),
        Command::Capabilities => (capabilities_response(), false),
        Command::Devfs => (devfs_response(), false),
        Command::Quit => (alloc::string::String::from("bye\r\n"), true),
        Command::Empty => (alloc::string::String::new(), false),
        Command::Unknown => {
            (alloc::string::String::from("unknown command; type 'help'\r\n"), false)
        }
    }
}

/// Return a bounded, read-only snapshot of the current software `/dev` view.
/// This reports cached node paths only; it does not probe hardware or grant a
/// shell. The output is intentionally capped for the line-oriented monitor.
fn devfs_response() -> alloc::string::String {
    let (generation, nodes) = fs::devfs::active_impl::snapshot();
    let mut paths = alloc::string::String::new();
    for (index, node) in nodes.iter().take(32).enumerate() {
        if index != 0 {
            paths.push(',');
        }
        paths.push_str(node.path.as_str());
    }
    let truncated = nodes.len() > 32;
    format!("devfs generation={} nodes={} truncated={} paths={}\r\n",
            generation,
            nodes.len(),
            truncated,
            paths)
}

#[cfg(feature = "loongson2k1000la")]
fn capabilities_response() -> alloc::string::String {
    let Some(snapshot) = driver::loongson2k1000_capability_snapshot() else {
        return alloc::string::String::from("capabilities unavailable\r\n");
    };
    format!("capabilities uart={} irq={} mmc={} dma={} devfs_generation={:?} states=uart:{:?},irq:{:?},mmc:{:?},dma:{:?},network:{:?},input:{:?}\r\n",
            snapshot.uart_count,
            snapshot.irq_controller_count,
            snapshot.mmc_count,
            snapshot.dma_controller_count,
            driver::loongson2k1000_devfs_generation(),
            snapshot.uart,
            snapshot.irq,
            snapshot.mmc,
            snapshot.dma,
            snapshot.network,
            snapshot.input)
}

#[cfg(not(feature = "loongson2k1000la"))]
fn capabilities_response() -> alloc::string::String {
    alloc::string::String::from("ERR unsupported: capabilities requires loongson2k1000la\r\n")
}

#[cfg(feature = "loongson2k1000la")]
fn ls2k_irq_response() -> alloc::string::String {
    let snapshot = driver::loongson2k1000_irq_diagnostic_snapshot();
    let Some(runtime) = snapshot.runtime else {
        return format!("ls2k-irq state={:?} runtime=unavailable\r\n",
                       snapshot.slot_state);
    };
    let failure = |bank : usize| match runtime.status_poll_failures[bank] {
        None => alloc::string::String::from("none"),
        Some(failure) => format!("{:?},{:#x},{:#x},{:#x},{}",
                                 failure.report
                                        .operation,
                                 failure.report
                                        .expected_mask,
                                 failure.report
                                        .expected_value,
                                 failure.report
                                        .observed_status,
                                 failure.report.polls),
    };
    format!("ls2k-irq state={:?} configured={:#x} parents={:#x} calls={} ok={} fail={} \
             parent_events={} masked={} handled={} unhandled={} rearmed={} bank0={} bank1={}\r\n",
            snapshot.slot_state,
            runtime.configured_sources,
            runtime.parent_lines,
            runtime.service
                   .calls,
            runtime.service
                   .successes,
            runtime.service
                   .failures,
            runtime.service
                   .parent_lines,
            runtime.service
                   .masked_sources,
            runtime.service
                   .handled_sources,
            runtime.service
                   .unhandled_sources,
            runtime.service
                   .rearmed_sources,
            failure(0),
            failure(1))
}

#[cfg(all(feature = "loongson2k1000la", target_arch = "loongarch64"))]
fn ls2k_mmc_response() -> alloc::string::String {
    let error_code = |error| match error {
        driver::Loongson2k1000MmcDiagnosticError::Busy => "busy",
        driver::Loongson2k1000MmcDiagnosticError::TopologyUnavailable => {
            "topology-unavailable"
        }
        driver::Loongson2k1000MmcDiagnosticError::HostCount => "invalid-host-count",
        driver::Loongson2k1000MmcDiagnosticError::InvalidPlan => "invalid-plan",
        driver::Loongson2k1000MmcDiagnosticError::ClockBackend => "clock-backend",
        driver::Loongson2k1000MmcDiagnosticError::GpioBackend => "gpio-backend",
        driver::Loongson2k1000MmcDiagnosticError::PinctrlBackend => "pinctrl-backend",
        driver::Loongson2k1000MmcDiagnosticError::ControllerBackend => "controller-backend",
    };
    // SAFETY: the 2K1000 platform initialized the topology mapping; the driver
    // one-shot gate excludes concurrent monitor reads. Hardware semantics are
    // still UNVERIFIED_ON_HARDWARE.
    match unsafe { driver::diagnose_loongson2k1000_mmc() } {
        Ok(response) => response,
        Err(error) => format!("ERR ls2k-mmc {}\r\n", error_code(error)),
    }
}

#[cfg(all(feature = "loongson2k1000la", not(target_arch = "loongarch64")))]
fn ls2k_mmc_response() -> alloc::string::String {
    alloc::string::String::from("ERR unavailable: ls2k-mmc requires loongarch64 target\r\n")
}

#[cfg(not(feature = "loongson2k1000la"))]
fn ls2k_mmc_response() -> alloc::string::String {
    alloc::string::String::from("ERR unsupported: ls2k-mmc requires loongson2k1000la\r\n")
}

#[cfg(not(feature = "loongson2k1000la"))]
fn ls2k_irq_response() -> alloc::string::String {
    alloc::string::String::from("ERR unsupported: ls2k-irq requires loongson2k1000la\r\n")
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
        assert_eq!(parse_command(b"ls2k-irq"),
                   Command::Ls2kIrq);
        assert_eq!(parse_command(b" caps "), Command::Capabilities);
        assert_eq!(parse_command(b" devfs "), Command::Devfs);
        assert_eq!(parse_command(b" ls2k-mmc "), Command::Ls2kMmc);
        assert_eq!(parse_command(b"\t"), Command::Empty);
    }

    #[test]
    fn command_parser_rejects_invalid_or_unknown_input() {
        assert_eq!(parse_command(b"reboot"),
                   Command::Unknown);
        assert_eq!(parse_command(&[0xFF]), Command::Unknown);
    }

    #[test]
    fn ls2k_irq_command_is_read_only_and_profile_gated() {
        let (response, close) = super::command_response(Command::Ls2kIrq);
        assert!(!close);
        #[cfg(feature = "loongson2k1000la")]
        assert!(response.starts_with("ls2k-irq state="));
        #[cfg(not(feature = "loongson2k1000la"))]
        assert_eq!(response,
                   "ERR unsupported: ls2k-irq requires loongson2k1000la\r\n");
    }

    #[test]
    fn ls2k_mmc_command_is_profile_gated_and_never_closes_session() {
        let (response, close) = super::command_response(Command::Ls2kMmc);
        assert!(!close);
        #[cfg(not(feature = "loongson2k1000la"))]
        assert_eq!(response,
                   "ERR unsupported: ls2k-mmc requires loongson2k1000la\r\n");
        #[cfg(all(feature = "loongson2k1000la", not(target_arch = "loongarch64")))]
        assert_eq!(response,
                   "ERR unavailable: ls2k-mmc requires loongarch64 target\r\n");
        #[cfg(all(feature = "loongson2k1000la", target_arch = "loongarch64"))]
        assert!(response.starts_with("ls2k-mmc ") || response.starts_with("ERR ls2k-mmc "));
    }

    #[test]
    fn capabilities_command_is_read_only_and_profile_gated() {
        let (response, close) = super::command_response(Command::Capabilities);
        assert!(!close);
        #[cfg(not(feature = "loongson2k1000la"))]
        assert_eq!(response,
                   "ERR unsupported: capabilities requires loongson2k1000la\r\n");
    }
}
