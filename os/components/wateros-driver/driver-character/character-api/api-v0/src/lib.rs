//! 字符设备 API（v0）：[`CharacterDevice`] 全局注册表与 [`SerialPort`] MMIO 辅助 trait。

#![no_std]

extern crate alloc;

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use driver_api::{DriverError, DriverResult};
use spin::Mutex;

pub use driver_api::{DriverError as CharacterDriverError, DriverResult as CharacterDriverResult};

/// 串行端口一次字节写入失败原因（忙等路径上多为发送保持寄存器未空）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerialError {
    /// 发送侧在合理自旋后仍不可写（硬件异常或误用寄存器布局）。
    TransmitterStuck,
}

/// 串行端口上的 `Result` 别名。
pub type SerialResult<T> = core::result::Result<T, SerialError>;

/// 最小串行 I/O 契约：MMIO UART 等底层实现；由 [`SerialPortCharacterDevice`] 包装为 [`CharacterDevice`]。
pub trait SerialPort: Send {
    /// 写单字节；发送前可自旋直至 THRE。
    fn write_byte(&mut self, byte: u8) -> SerialResult<()>;

    /// 顺序写入缓冲区。
    fn write_all(&mut self, bytes: &[u8]) -> SerialResult<()> {
        for &b in bytes {
            self.write_byte(b)?;
        }
        Ok(())
    }

    /// 阻塞直到收到一字节（自旋轮询 DR）。
    fn read_byte_blocking(&mut self) -> u8;

    /// 无数据时返回 `None`，不阻塞。
    fn try_read_byte(&mut self) -> Option<u8>;
}

/// 字符设备类别（供 devfs 路径别名与 syscall 分发）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterDeviceKind {
    Serial,
    Rtc,
    Null,
}

/// 可在多任务间共享的字符设备句柄。
pub type SharedCharacterDevice = Arc<Mutex<Box<dyn CharacterDevice>>>;

static CHARACTER_DEVICES: Mutex<Vec<SharedCharacterDevice>> = Mutex::new(Vec::new());

/// 字符设备语义：字节流 I/O + 可选 `ioctl`（默认 [`DriverError::Unsupported`]）。
pub trait CharacterDevice: Send {
    /// 从设备读入 `buf`；返回实际字节数，0 表示 EOF（由实现定义）。
    fn read(&mut self, buf: &mut [u8]) -> DriverResult<usize>;

    /// 向设备写出 `buf`；返回已写字节数。
    fn write(&mut self, buf: &[u8]) -> DriverResult<usize>;

    /// `poll` 语义：根据请求的 `events` 掩码返回就绪的 `revents`。
    fn poll_revents(&mut self, events: i16) -> DriverResult<i16> {
        const POLLIN: i16 = 0x001;
        const POLLOUT: i16 = 0x004;
        let mut revents = 0i16;
        if events & POLLIN != 0 {
            revents |= POLLIN;
        }
        if events & POLLOUT != 0 {
            revents |= POLLOUT;
        }
        Ok(revents)
    }

    fn ioctl(&mut self, _request: usize, _arg: usize) -> DriverResult<isize> {
        Err(DriverError::Unsupported)
    }

    /// devfs 与 VFS 用于区分 UART 与 RTC 等设备。
    fn device_kind(&self) -> CharacterDeviceKind {
        CharacterDeviceKind::Serial
    }
}

/// 将 [`SerialPort`] 包装为 [`CharacterDevice`]（非阻塞读；无数据时返回 [`DriverError::Unsupported`]）。
pub struct SerialPortCharacterDevice<P: SerialPort> {
    port: P,
}

impl<P: SerialPort> SerialPortCharacterDevice<P> {
    pub fn new(port: P) -> Self {
        Self { port }
    }
}

impl<P: SerialPort> CharacterDevice for SerialPortCharacterDevice<P> {
    fn read(&mut self, buf: &mut [u8]) -> DriverResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if let Some(b) = self.port.try_read_byte() {
            buf[0] = b;
            return Ok(1);
        }
        Err(DriverError::Unsupported)
    }

    fn write(&mut self, buf: &[u8]) -> DriverResult<usize> {
        self.port
            .write_all(buf)
            .map_err(|_| DriverError::IoError)?;
        Ok(buf.len())
    }

    fn poll_revents(&mut self, events: i16) -> DriverResult<i16> {
        const POLLIN: i16 = 0x001;
        const POLLOUT: i16 = 0x004;
        let mut revents = 0i16;
        if events & POLLIN != 0 && self.port.try_read_byte().is_some() {
            revents |= POLLIN;
        }
        if events & POLLOUT != 0 {
            revents |= POLLOUT;
        }
        Ok(revents)
    }

}

/// 将设备追加到全局表末尾，返回其索引（从 0 起）。
pub fn register_character_device(device: SharedCharacterDevice) -> usize {
    let mut devices = CHARACTER_DEVICES.lock();
    devices.push(device);
    devices.len() - 1
}

/// 当前已注册字符设备数量。
pub fn character_device_count() -> usize {
    CHARACTER_DEVICES.lock().len()
}

/// 按下标取设备；越界返回 `None`。
pub fn character_device_at(index: usize) -> Option<SharedCharacterDevice> {
    CHARACTER_DEVICES.lock().get(index).cloned()
}

/// 取首个字符设备。
pub fn first_character_device() -> Option<SharedCharacterDevice> {
    CHARACTER_DEVICES.lock().first().cloned()
}

/// 对指定下标设备加锁并执行 `f`。
pub fn with_character_device<F, R>(index: usize, f: F) -> Option<R>
where
    F: FnOnce(&mut dyn CharacterDevice) -> R,
{
    let dev = character_device_at(index)?;
    let mut guard = dev.lock();
    Some(f(guard.as_mut()))
}

/// 查询指定下标设备的类别。
pub fn character_device_kind_at(index: usize) -> Option<CharacterDeviceKind> {
    with_character_device(index, |dev| dev.device_kind())
}
