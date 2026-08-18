//! UNIX98 PTY 的 VFS 打开文件描述实现。

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use api_v0::{
    VfsCopyProgress, VfsError, VfsIoHandle, VfsMetadata, VfsNodeType, VfsPreparedRead,
    VfsReadFinish, VfsReadLease, VfsResult, VfsSpecialDeviceInfo, VfsTerminalEndpoint,
    VfsTerminalInfo,
};
use tty::{PtyEndpointHandle, PtyError, PtyPreparedRead, PtyReadReservation, TerminalEndpoint};

const POLLIN: i16 = 0x001;
const POLLOUT: i16 = 0x004;
const POLLERR: i16 = 0x008;
const POLLHUP: i16 = 0x010;
const O_NONBLOCK: u32 = 0o4000;

fn map_error(error: PtyError) -> VfsError {
    match error {
        PtyError::NotFound => VfsError::NotFound,
        PtyError::Locked => VfsError::AccessDenied,
        PtyError::NoSpace => VfsError::NoSpace,
        PtyError::Invalid => VfsError::InvalidPath,
        PtyError::HungUp => VfsError::Io,
        PtyError::WouldBlock => VfsError::WouldBlock,
        PtyError::Interrupted => VfsError::Interrupted,
        PtyError::Busy => VfsError::Busy,
    }
}

fn path_inode(path: &str) -> u64 {
    let mut hash = 0xCBF2_9CE4_8422_2325u64;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01B3);
    }
    hash | (1 << 63)
}

fn metadata(path: &str, mode: u16, major: u32, minor: u32) -> VfsMetadata {
    VfsMetadata {
        node_type: VfsNodeType::Special,
        size: 0,
        mode,
        device_major: major,
        device_minor: minor,
        inode: path_inode(path),
        mount_id: 0,
        nlink: 1,
        uid: 0,
        gid: 0,
    }
}

fn slave_metadata(path: &str, number: u32) -> VfsMetadata {
    let mut value = metadata(path, 0o20620, 136, number);
    value.gid = 5; // Linux 约定的 tty 组；base-layout 同步提供 tty:x:5。
    value
}

struct PtyVfsPreparedRead {
    /// PTY 端点的共享句柄。
    endpoint: PtyEndpointHandle,
    /// 本次最多读取字节数。
    max_len: usize,
}

impl VfsPreparedRead for PtyVfsPreparedRead {
    fn acquire(self: Box<Self>) -> VfsResult<Box<dyn VfsReadLease>> {
        loop {
            match self.endpoint.prepare_read(self.max_len, false) {
                PtyPreparedRead::Data(reservation) => {
                    return Ok(Box::new(PtyVfsReadLease { reservation: Some(reservation) }));
                }
                PtyPreparedRead::Eof => return Ok(Box::new(PtyEmptyReadLease)),
                PtyPreparedRead::HungUp => return Err(VfsError::Io),
                PtyPreparedRead::Pending => {}
            }
            if self.endpoint.nonblocking() { return Err(VfsError::WouldBlock); }

            let (canonical, minimum, deciseconds) = self.endpoint.read_settings();
            let buffered = self.endpoint.readable_len();
            let result = if self.endpoint.endpoint() == TerminalEndpoint::PtyMaster ||
                            canonical || deciseconds == 0 {
                self.endpoint.wait_readable(self.max_len)
            } else {
                let tick_ms = base_config::task::SCHED_TIMER_PERIOD_MS.max(1);
                let timeout_ms = deciseconds.saturating_mul(100);
                let ticks = timeout_ms.saturating_add(tick_ms - 1) / tick_ms;
                self.endpoint.wait_readable_for_ticks(self.max_len, ticks.max(1))
            };
            match result {
                // 被信号打断直接传播 EINTR 语义，超时则按 termios 最小字节数决定 EOF。
                waitqueue::TaskWaitResult::Interrupted => return Err(VfsError::Interrupted),
                waitqueue::TaskWaitResult::TimedOut => {
                    if minimum == 0 && buffered == 0 {
                        return Ok(Box::new(PtyEmptyReadLease));
                    }
                    if let PtyPreparedRead::Data(reservation) =
                        self.endpoint.prepare_read(self.max_len, true)
                    {
                        return Ok(Box::new(PtyVfsReadLease { reservation: Some(reservation) }));
                    }
                }
                waitqueue::TaskWaitResult::Woken => {}
            }
        }
    }
}

struct PtyVfsReadLease { reservation: Option<PtyReadReservation> }

impl VfsReadLease for PtyVfsReadLease {
    fn bytes(&self) -> &[u8] {
        self.reservation.as_ref().map(PtyReadReservation::bytes).unwrap_or(&[])
    }

    fn finish(mut self: Box<Self>, progress: VfsCopyProgress) -> VfsResult<VfsReadFinish> {
        let reservation = self.reservation.take().ok_or(VfsError::Io)?;
        reservation.finish(progress.copied, progress.complete)
            .map(VfsReadFinish::Bytes).map_err(map_error)
    }
}

struct PtyEmptyReadLease;
impl VfsReadLease for PtyEmptyReadLease {
    fn bytes(&self) -> &[u8] { &[] }
    fn finish(self: Box<Self>, progress: VfsCopyProgress) -> VfsResult<VfsReadFinish> {
        if progress.copied == 0 { Ok(VfsReadFinish::Bytes(0)) } else { Err(VfsError::Io) }
    }
}

/// `/dev/ptmx` 或 `/dev/pts/N` 的打开文件描述。
pub struct PtyVfsHandle { endpoint: PtyEndpointHandle }

impl PtyVfsHandle {
    fn new(endpoint: PtyEndpointHandle) -> Self { Self { endpoint } }
    pub fn endpoint(&self) -> &PtyEndpointHandle { &self.endpoint }
}

impl VfsIoHandle for PtyVfsHandle {
    fn prepare_read(&mut self, max_len: usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        if self.endpoint.accmode() == 1 { return Err(VfsError::BadFd); }
        Ok(Box::new(PtyVfsPreparedRead { endpoint: self.endpoint.clone(), max_len }))
    }

    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        let lease = self.prepare_read(buf.len())?.acquire()?;
        let len = lease.bytes().len();
        buf[..len].copy_from_slice(lease.bytes());
        match lease.finish(VfsCopyProgress { copied: len, complete: true })? {
            VfsReadFinish::Bytes(copied) => Ok(copied),
            VfsReadFinish::Fault => Err(VfsError::Io),
        }
    }

    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        if self.endpoint.accmode() == 0 { return Err(VfsError::BadFd); }
        loop {
            match self.endpoint.write(buf) {
                Ok(written) => return Ok(written),
                Err(PtyError::WouldBlock) if !self.endpoint.nonblocking() => {
                    if self.endpoint.wait_writable() == waitqueue::TaskWaitResult::Interrupted {
                        return Err(VfsError::Interrupted);
                    }
                }
                Err(error) => return Err(map_error(error)),
            }
        }
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        match self.endpoint.endpoint() {
            TerminalEndpoint::PtyMaster => Ok(metadata("/dev/ptmx", 0o20666, 5, 2)),
            TerminalEndpoint::PtySlave => {
                let path = format!("/dev/pts/{}", self.endpoint.number());
                Ok(slave_metadata(path.as_str(), self.endpoint.number()))
            }
            TerminalEndpoint::Console => Err(VfsError::InvalidPath),
        }
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self::new(self.endpoint.clone())))
    }

    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
        let mut result = 0;
        if events & POLLIN != 0 && self.endpoint.poll_readable() { result |= POLLIN; }
        if events & POLLOUT != 0 && self.endpoint.poll_writable() { result |= POLLOUT; }
        if self.endpoint.poll_hung_up() { result |= POLLHUP; }
        Ok(result & (events | POLLERR | POLLHUP))
    }

    fn poll_wait_for_ticks(&mut self, events: i16, timeout_ticks: u64,
                           still_waiting: &mut dyn FnMut() -> bool) -> VfsResult<()> {
        for _ in 0..timeout_ticks.max(1) {
            if !still_waiting() || self.poll_revents(events)? != 0 { return Ok(()); }
            task::sleep_for_ticks(1);
        }
        Ok(())
    }

    fn special_device_info(&self) -> Option<VfsSpecialDeviceInfo> {
        let endpoint = match self.endpoint.endpoint() {
            TerminalEndpoint::PtyMaster => VfsTerminalEndpoint::PtyMaster,
            TerminalEndpoint::PtySlave => VfsTerminalEndpoint::PtySlave,
            TerminalEndpoint::Console => VfsTerminalEndpoint::Console,
        };
        Some(VfsSpecialDeviceInfo::Terminal(VfsTerminalInfo {
            id: self.endpoint.id().raw(), endpoint, pty_number: Some(self.endpoint.number()),
        }))
    }

    fn is_tty_char_device(&self) -> bool { true }
    fn open_accmode(&self) -> u32 { self.endpoint.accmode() }
    fn open_status_flags(&self) -> u32 { if self.endpoint.nonblocking() { O_NONBLOCK } else { 0 } }
    fn set_open_status_flags(&mut self, flags: u32) -> VfsResult<()> {
        self.endpoint.set_nonblocking(flags & O_NONBLOCK != 0);
        Ok(())
    }
}

pub fn pty_special_device_paths() -> alloc::vec::Vec<String> {
    let mut paths = alloc::vec!["/dev/ptmx".to_string(), "/dev/tty".to_string()];
    paths.extend(tty::pty_numbers().into_iter().map(|number| format!("/dev/pts/{number}")));
    paths
}

pub fn pty_special_device_exists(path: &str) -> bool {
    path == "/dev/ptmx" || path == "/dev/tty" ||
        path.strip_prefix("/dev/pts/").and_then(|value| value.parse::<u32>().ok())
            .is_some_and(|number| tty::pty_numbers().contains(&number))
}

pub fn pty_special_device_metadata(path: &str) -> Option<VfsMetadata> {
    if path == "/dev/ptmx" { return Some(metadata(path, 0o20666, 5, 2)); }
    if path == "/dev/tty" { return Some(metadata(path, 0o20666, 5, 0)); }
    let number = path.strip_prefix("/dev/pts/")?.parse::<u32>().ok()?;
    tty::pty_numbers().contains(&number).then(|| slave_metadata(path, number))
}

pub fn open_pty_special_device(path: &str, accmode: u32, nonblocking: bool,
                               current_sid: Option<usize>)
    -> Option<VfsResult<Box<dyn VfsIoHandle>>> {
    let result = if path == "/dev/ptmx" {
        tty::allocate_pty(accmode, nonblocking)
    } else if path == "/dev/tty" {
        let sid = match current_sid { Some(sid) => sid, None => return Some(Err(VfsError::NoDevice)) };
        match tty::terminal_for_session(sid, accmode, nonblocking) {
            Ok(endpoint) => return Some(Ok(Box::new(PtyVfsHandle::new(endpoint)))),
            Err(PtyError::NotFound) if tty::controlling_sid() == sid => {
                return Some(crate::registry::open_console_tty(accmode).ok_or(VfsError::NoDevice));
            }
            Err(error) => Err(error),
        }
    } else if let Some(number) = path.strip_prefix("/dev/pts/")
        .and_then(|value| value.parse::<u32>().ok()) {
        tty::open_pty_slave(number, accmode, nonblocking)
    } else {
        return None;
    };
    Some(result.map(|endpoint| Box::new(PtyVfsHandle::new(endpoint)) as Box<dyn VfsIoHandle>)
        .map_err(map_error))
}
