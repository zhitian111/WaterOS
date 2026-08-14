//! 字符设备 [`VfsIoHandle`]：包装 [`SharedCharacterDevice`]。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::boxed::Box;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering};

use api_v0::{
    VfsCopyProgress, VfsError, VfsIoHandle, VfsMetadata, VfsNodeType, VfsPreparedRead,
    VfsReadFinish, VfsReadLease, VfsResult,
};
use driver_api::DriverError;
use driver_character_api_v0::{
    CharacterReadFinish, CharacterReadReservation, SharedCharacterDevice,
};
use tty::{self, TtyPreparedRead, TtyReadReservation};

// 本方法代码由AI完成
fn map_driver_err(e: DriverError) -> VfsError {
    match e {
        DriverError::Unsupported => VfsError::Unsupported,
        DriverError::InvalidParam => VfsError::InvalidPath,
        DriverError::NotFound => VfsError::NotFound,
        DriverError::InvalidDtb | DriverError::IoError => VfsError::Io,
    }
}

// 本方法代码由AI完成
fn char_metadata(mode: u16, inode: u64) -> VfsMetadata {
    VfsMetadata {
        node_type: VfsNodeType::Special,
        size: 0,
        mode,
        device_major: 0,
        device_minor: 0x7FFF_0001,
        inode,
        mount_id: 0,
        nlink: 1,
        uid: 0,
        gid: 0,
    }
}

// 本方法代码由AI完成
fn path_inode(path: &str) -> u64 {
    let mut hash = 0xCBF2_9CE4_8422_2325u64;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01B3);
    }
    hash | (1u64 << 63)
}

/// 已打开的字符设备句柄。
// 本结构代码由AI完成
pub struct CharDevHandle {
    device: SharedCharacterDevice,
    read_eof: bool,
    rtc: bool,
    tty: bool,
    tty_input: bool,
    tty_output: bool,
    nonblocking: Arc<AtomicBool>,
    accmode: u32,
    mode: u16,
    inode: u64,
}

impl CharDevHandle {
// 本方法代码由AI完成
    pub fn new(device: SharedCharacterDevice, input: bool) -> Self {
        Self {
            device,
            read_eof: false,
            rtc: false,
            tty: true,
            tty_input: input,
            tty_output: !input,
            nonblocking: Arc::new(AtomicBool::new(false)),
            accmode: if input { 0 } else { 1 },
            mode: if input { 0o20600 } else { 0o20660 },
            inode: if input { 1 } else { 2 },
        }
    }

// 本方法代码由AI完成
    pub fn new_rtc(device: SharedCharacterDevice) -> Self {
        Self {
            device,
            read_eof: true,
            rtc: true,
            tty: false,
            tty_input: false,
            tty_output: false,
            nonblocking: Arc::new(AtomicBool::new(false)),
            accmode: 2,
            mode: 0o20644,
            inode: path_inode("/dev/rtc0"),
        }
    }

// 本方法代码由AI完成
    pub fn from_devfs_path(device: SharedCharacterDevice, path: &str, accmode: u32) -> Self {
        if path == "/dev/null" {
            Self {
                device,
                read_eof: true,
                rtc: false,
                tty: false,
                tty_input: false,
                tty_output: false,
                nonblocking: Arc::new(AtomicBool::new(false)),
                accmode,
                mode: 0o20666,
                inode: path_inode(path),
            }
        } else if is_rtc_dev_path(path) {
            let mut handle = Self::new_rtc(device);
            handle.inode = path_inode(path);
            handle.accmode = accmode;
            handle
        } else {
            let mut handle = Self::new(device, accmode != 1);
            handle.accmode = accmode;
            handle.inode = path_inode(path);
            handle.tty_output = accmode != 0;
            handle
        }
    }

// 本方法代码由AI完成
    pub fn new_stdin(device: SharedCharacterDevice) -> Self {
        Self::new(device, true)
    }

// 本方法代码由AI完成
    pub fn new_stdout(device: SharedCharacterDevice) -> Self {
        Self::new(device, false)
    }
}

// 本方法代码由AI完成
fn serial_poll_revents(device: &SharedCharacterDevice, events: i16) -> VfsResult<i16> {
// 本变量代码由AI完成
// 本变量代码由AI完成
    const POLLOUT: i16 = 0x004;
    let mut guard = device.lock();
    let mut revents = guard.poll_revents(events).map_err(map_driver_err)?;
    drop(guard);
    if events & POLLOUT != 0 {
        revents |= POLLOUT;
    }
    Ok(revents)
}

struct CharDevPreparedRead {
    device: SharedCharacterDevice,
    read_eof: bool,
    rtc: bool,
    tty_input: bool,
    nonblocking: Arc<AtomicBool>,
    max_len: usize,
}

impl VfsPreparedRead for CharDevPreparedRead {
    fn acquire(self: Box<Self>) -> VfsResult<Box<dyn VfsReadLease>> {
        loop {
            if self.tty_input {
                match tty::prepare_read(self.max_len) {
                    TtyPreparedRead::Data(reservation) => {
                        return Ok(Box::new(TtyVfsReadLease { reservation: Some(reservation) }));
                    }
                    TtyPreparedRead::Eof => return Ok(Box::new(EmptyCharacterReadLease)),
                    TtyPreparedRead::Pending => {}
                }
                if self.nonblocking.load(Ordering::Acquire) {
                    return Err(VfsError::WouldBlock);
                }
                let (canonical, minimum, deciseconds) = tty::read_settings();
                let buffered = tty::readable_len();
                let wait_result = if canonical || deciseconds == 0 {
                    tty::wait_for_input(self.max_len)
                } else if minimum != 0 && buffered == 0 {
                    tty::wait_for_input_change(0)
                } else {
                    let tick_ms = base_config::task::SCHED_TIMER_PERIOD_MS.max(1);
                    let timeout_ms = deciseconds.saturating_mul(100);
                    let timeout_ticks = timeout_ms.saturating_add(tick_ms - 1) / tick_ms;
                    tty::wait_for_input_change_for_ticks(buffered,
                                                         timeout_ticks.max(1))
                };
                match wait_result {
                    waitqueue::TaskWaitResult::Interrupted => return Err(VfsError::Interrupted),
                    waitqueue::TaskWaitResult::TimedOut => {
                        if minimum == 0 {
                            return Ok(Box::new(EmptyCharacterReadLease));
                        }
                        if let TtyPreparedRead::Data(reservation) =
                            tty::prepare_partial_read(self.max_len)
                        {
                            return Ok(Box::new(TtyVfsReadLease {
                                reservation: Some(reservation),
                            }));
                        }
                    }
                    waitqueue::TaskWaitResult::Woken => {}
                }
                continue;
            }
            let prepared = self.device.lock().prepare_read(self.max_len);
            match prepared {
                Ok(Some(reservation)) => {
                    return Ok(Box::new(CharacterDeviceVfsReadLease {
                        device: self.device.clone(),
                        reservation: Some(reservation),
                    }));
                }
                Ok(None) => {}
                Err(DriverError::Unsupported) if self.rtc || self.read_eof => {
                    return Ok(Box::new(EmptyCharacterReadLease));
                }
                Err(error) => return Err(map_driver_err(error)),
            }
            if self.read_eof || self.rtc {
                return Ok(Box::new(EmptyCharacterReadLease));
            }
            if self.nonblocking.load(Ordering::Acquire) {
                return Err(VfsError::WouldBlock);
            }
            task::yield_now();
        }
    }
}

struct EmptyCharacterReadLease;

impl VfsReadLease for EmptyCharacterReadLease {
    fn bytes(&self) -> &[u8] { &[] }

    fn finish(self: Box<Self>, progress: VfsCopyProgress) -> VfsResult<VfsReadFinish> {
        if progress.copied != 0 {
            return Err(VfsError::Io);
        }
        Ok(VfsReadFinish::Bytes(0))
    }
}

struct CharacterDeviceVfsReadLease {
    device: SharedCharacterDevice,
    reservation: Option<CharacterReadReservation>,
}

impl VfsReadLease for CharacterDeviceVfsReadLease {
    fn bytes(&self) -> &[u8] {
        self.reservation.as_ref()
                        .map(CharacterReadReservation::bytes)
                        .unwrap_or(&[])
    }

    fn finish(mut self: Box<Self>, progress: VfsCopyProgress) -> VfsResult<VfsReadFinish> {
        let reservation = self.reservation.take().ok_or(VfsError::Io)?;
        match self.device.lock()
                         .finish_read(reservation, progress.copied, progress.complete)
                         .map_err(map_driver_err)? {
            CharacterReadFinish::Bytes(copied) => Ok(VfsReadFinish::Bytes(copied)),
            CharacterReadFinish::Fault => Ok(VfsReadFinish::Fault),
        }
    }
}

impl Drop for CharacterDeviceVfsReadLease {
    fn drop(&mut self) {
        if let Some(reservation) = self.reservation.take() {
            let _ = self.device.lock().finish_read(reservation, 0, false);
        }
    }
}

struct TtyVfsReadLease {
    reservation: Option<TtyReadReservation>,
}

impl VfsReadLease for TtyVfsReadLease {
    fn bytes(&self) -> &[u8] {
        self.reservation.as_ref()
                        .map(TtyReadReservation::bytes)
                        .unwrap_or(&[])
    }

    fn finish(mut self: Box<Self>, progress: VfsCopyProgress) -> VfsResult<VfsReadFinish> {
        tty::finish_read(self.reservation.take().ok_or(VfsError::Io)?,
                         progress.copied,
                         progress.complete)
            .map(VfsReadFinish::Bytes)
            .map_err(|_| VfsError::Io)
    }
}

impl Drop for TtyVfsReadLease {
    fn drop(&mut self) {
        if let Some(reservation) = self.reservation.take() {
            let _ = tty::finish_read(reservation, 0, false);
        }
    }
}

pub(crate) fn read_lease_self_test() {
    tty::configure(tty::ConsoleTtyMode::Fixture);
    let TtyPreparedRead::Data(read) = tty::prepare_read(3) else {
        panic!("fixture must be readable");
    };
    assert_eq!(read.bytes(), b"pas");
    assert_eq!(tty::finish_read(read, 1, true), Ok(1));
    tty::configure(tty::ConsoleTtyMode::Closed);
}

impl VfsIoHandle for CharDevHandle {
    fn prepare_read(&mut self, max_len: usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        Ok(Box::new(CharDevPreparedRead {
            device: self.device.clone(),
            read_eof: self.read_eof,
            rtc: self.rtc,
            tty_input: self.tty_input,
            nonblocking: self.nonblocking.clone(),
            max_len,
        }))
    }

// 本方法代码由AI完成
    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        let prepared = self.prepare_read(buf.len())?;
        let lease = prepared.acquire()?;
        let len = lease.bytes().len();
        buf[..len].copy_from_slice(lease.bytes());
        match lease.finish(VfsCopyProgress { copied: len, complete: true })? {
            VfsReadFinish::Bytes(copied) => Ok(copied),
            VfsReadFinish::Fault => Err(VfsError::Io),
        }
    }

// 本方法代码由AI完成
    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        if self.tty_output {
            let output = tty::transform_output(buf);
            console::write_raw_bytes(&output);
            return Ok(buf.len());
        }
        let mut guard = self.device.lock();
        guard.write(buf).map_err(map_driver_err)
    }

// 本方法代码由AI完成
    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
        if self.rtc {
            let mut guard = self.device.lock();
            return guard.poll_revents(events).map_err(map_driver_err);
        }
        if self.read_eof {
// 本变量代码由AI完成
            const POLLIN: i16 = 0x001;
// 本变量代码由AI完成
            const POLLOUT: i16 = 0x004;
            let mut revents = 0i16;
            if events & POLLIN != 0 {
                revents |= POLLIN;
            }
            if events & POLLOUT != 0 {
                revents |= POLLOUT;
            }
            return Ok(revents);
        }
        if self.tty_input {
            const POLLIN: i16 = 0x001;
            const POLLOUT: i16 = 0x004;
            let mut revents = 0;
            if events & POLLIN != 0 && tty::poll_readable() {
                revents |= POLLIN;
            }
            if events & POLLOUT != 0 && self.tty_output {
                revents |= POLLOUT;
            }
            return Ok(revents);
        }
        serial_poll_revents(&self.device, events)
    }

// 本方法代码由AI完成
    fn poll_wait_for_ticks(
        &mut self,
        events: i16,
        timeout_ticks: u64,
        still_waiting: &mut dyn FnMut() -> bool,
    ) -> VfsResult<()> {
        if self.rtc || self.read_eof {
            return Err(VfsError::Unsupported);
        }
// 本变量代码由AI完成
        const POLLIN: i16 = 0x001;
        if events & POLLIN == 0 {
            return Ok(());
        }
        for _ in 0..timeout_ticks.max(1) {
            if !still_waiting() {
                return Ok(());
            }
            let readable = if self.tty_input {
                tty::poll_readable()
            } else {
                (serial_poll_revents(&self.device, POLLIN)? & POLLIN) != 0
            };
            if readable {
                return Ok(());
            }
            task::yield_now();
        }
        Ok(())
    }

// 本方法代码由AI完成
    fn ioctl(&mut self, request: usize, arg: usize) -> VfsResult<isize> {
        if !self.rtc {
            return Err(VfsError::Unsupported);
        }
        let mut guard = self.device.lock();
        guard.ioctl(request, arg).map_err(map_driver_err)
    }

    fn is_rtc_device(&self) -> bool {
        self.rtc
    }

    fn is_tty_char_device(&self) -> bool {
        self.tty
    }

// 本方法代码由AI完成
    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(char_metadata(self.mode, self.inode))
    }

// 本方法代码由AI完成
    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self {
            device: self.device.clone(),
            read_eof: self.read_eof,
            rtc: self.rtc,
            tty: self.tty,
            tty_input: self.tty_input,
            tty_output: self.tty_output,
            nonblocking: self.nonblocking.clone(),
            accmode: self.accmode,
            mode: self.mode,
            inode: self.inode,
        }))
    }

// 本方法代码由AI完成
    fn open_status_flags(&self) -> u32 {
        if self.nonblocking.load(Ordering::Acquire) {
            0o0004000
        } else {
            0
        }
    }

    fn open_accmode(&self) -> u32 { self.accmode }

// 本方法代码由AI完成
    fn set_open_status_flags(&mut self, flags: u32) -> VfsResult<()> {
        self.nonblocking.store(flags & 0o0004000 != 0, Ordering::Release);
        Ok(())
    }
}

/// 若 `path` 为 RTC 别名则返回 true。
pub fn is_rtc_dev_path(path: &str) -> bool {
    matches!(path, "/dev/misc/rtc" | "/dev/rtc0" | "/dev/rtc")
}

// 本方法代码由AI完成
fn mode_for_devfs_path(path: &str) -> u16 {
    if path == "/dev/null" {
        0o20666
    } else if path == "/dev/random" || path == "/dev/urandom" {
        0o20666
    } else if path == "/dev/cpu_dma_latency" {
        0o20600
    } else if is_rtc_dev_path(path) {
        0o20644
    } else {
        0o20660
    }
}

/// 未打开 fd 时按 devfs 路径返回字符设备元数据（`fstatat` / `faccessat`）。
// 本方法代码由AI完成
pub fn metadata_for_devfs_path(path: &str) -> VfsMetadata {
    char_metadata(mode_for_devfs_path(path), path_inode(path))
}
