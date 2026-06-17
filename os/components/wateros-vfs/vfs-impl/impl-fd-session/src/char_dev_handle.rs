//! 字符设备 [`VfsIoHandle`]：包装 [`SharedCharacterDevice`]。

extern crate alloc;

use alloc::boxed::Box;

use api_v0::{VfsError, VfsIoHandle, VfsMetadata, VfsNodeType, VfsResult};
use driver_api::DriverError;
use driver_character_api_v0::SharedCharacterDevice;

fn map_driver_err(e: DriverError) -> VfsError {
    match e {
        DriverError::Unsupported => VfsError::Unsupported,
        DriverError::InvalidParam => VfsError::InvalidPath,
        DriverError::NotFound => VfsError::NotFound,
        DriverError::InvalidDtb | DriverError::IoError => VfsError::Io,
    }
}

fn char_metadata(mode: u16, inode: u64) -> VfsMetadata {
    VfsMetadata {
        node_type: VfsNodeType::Special,
        size: 0,
        mode,
        device_major: 0,
        device_minor: 0x7fff_0001,
        inode,
        mount_id: 0,
        nlink: 1,
    }
}

fn path_inode(path: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash | (1u64 << 63)
}

/// 已打开的字符设备句柄。
pub struct CharDevHandle {
    device: SharedCharacterDevice,
    stdin_eof: bool,
    rtc: bool,
    mode: u16,
    inode: u64,
}

impl CharDevHandle {
    pub fn new(device: SharedCharacterDevice, stdin_eof: bool) -> Self {
        Self {
            device,
            stdin_eof,
            rtc: false,
            mode: if stdin_eof { 0o20600 } else { 0o20660 },
            inode: if stdin_eof { 1 } else { 2 },
        }
    }

    pub fn new_rtc(device: SharedCharacterDevice) -> Self {
        Self {
            device,
            stdin_eof: false,
            rtc: true,
            mode: 0o20644,
            inode: path_inode("/dev/rtc0"),
        }
    }

    pub fn from_devfs_path(device: SharedCharacterDevice, path: &str) -> Self {
        if path == "/dev/null" {
            Self {
                device,
                stdin_eof: false,
                rtc: false,
                mode: 0o20666,
                inode: path_inode(path),
            }
        } else if is_rtc_dev_path(path) {
            let mut handle = Self::new_rtc(device);
            handle.inode = path_inode(path);
            handle
        } else {
            let mut handle = Self::new(device, false);
            handle.inode = path_inode(path);
            handle
        }
    }

    pub fn new_stdin(device: SharedCharacterDevice) -> Self {
        Self::new(device, true)
    }

    pub fn new_stdout(device: SharedCharacterDevice) -> Self {
        Self::new(device, false)
    }
}

impl VfsIoHandle for CharDevHandle {
    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut guard = self.device.lock();
        match guard.read(buf) {
            Ok(n) => Ok(n),
            Err(DriverError::Unsupported) if self.stdin_eof => Ok(0),
            Err(e) => Err(map_driver_err(e)),
        }
    }

    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        let mut guard = self.device.lock();
        guard.write(buf).map_err(map_driver_err)
    }

    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
        let mut guard = self.device.lock();
        guard.poll_revents(events).map_err(map_driver_err)
    }

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
        !self.rtc
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(char_metadata(self.mode, self.inode))
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self {
            device: self.device.clone(),
            stdin_eof: self.stdin_eof,
            rtc: self.rtc,
            mode: self.mode,
            inode: self.inode,
        }))
    }
}

/// 若 `path` 为 RTC 别名则返回 true。
pub fn is_rtc_dev_path(path: &str) -> bool {
    matches!(path, "/dev/misc/rtc" | "/dev/rtc0" | "/dev/rtc")
}

fn mode_for_devfs_path(path: &str) -> u16 {
    if path == "/dev/null" {
        0o20666
    } else if is_rtc_dev_path(path) {
        0o20644
    } else {
        0o20660
    }
}

/// 未打开 fd 时按 devfs 路径返回字符设备元数据（`fstatat` / `faccessat`）。
pub fn metadata_for_devfs_path(path: &str) -> VfsMetadata {
    char_metadata(mode_for_devfs_path(path), path_inode(path))
}
