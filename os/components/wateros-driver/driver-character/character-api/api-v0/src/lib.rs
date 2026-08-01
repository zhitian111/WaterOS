//! 字符设备 API（v0）：[`CharacterDevice`] 全局注册表与 [`SerialPort`] MMIO 辅助 trait。

#![no_std]

extern crate alloc;

use alloc::{boxed::Box, collections::VecDeque, sync::Arc, vec::Vec};
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

/// Bytes reserved from a consuming character device but not yet committed.
pub struct CharacterReadReservation {
    id: u64,
    bytes: Vec<u8>,
}

impl CharacterReadReservation {
    /// Construct a reservation owned by one character-device implementation.
    pub fn new(id: u64, bytes: Vec<u8>) -> Self {
        Self { id, bytes }
    }

    /// Stable bytes exposed while the device lock is not held.
    pub fn bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }

    /// Return the implementation token and staged bytes for commit or rollback.
    pub fn into_parts(self) -> (u64, Vec<u8>) {
        (self.id, self.bytes)
    }
}

/// Result of committing or cancelling a character-device read reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterReadFinish {
    /// This stream prefix was committed.
    Bytes(usize),
    /// No byte was committed because user-copy faulted immediately.
    Fault,
}

/// 字符设备语义：字节流 I/O + 可选 `ioctl`（默认 [`DriverError::Unsupported`]）。
pub trait CharacterDevice: Send {
    /// Reserve up to `max_len` bytes without making them unavailable on cancel.
    ///
    /// `Ok(None)` means that a transactional device currently has no data.
    /// Non-consuming devices may retain the default `Unsupported` result.
    fn prepare_read(&mut self, _max_len: usize) -> DriverResult<Option<CharacterReadReservation>> {
        Err(DriverError::Unsupported)
    }

    /// Commit the copied prefix and restore any uncommitted suffix in order.
    fn finish_read(
        &mut self,
        _reservation: CharacterReadReservation,
        _copied: usize,
        _complete: bool,
    ) -> DriverResult<CharacterReadFinish> {
        Err(DriverError::Unsupported)
    }

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
    pending: VecDeque<u8>,
    active_read: Option<u64>,
    next_read_id: u64,
}

impl<P: SerialPort> SerialPortCharacterDevice<P> {
    pub fn new(port: P) -> Self {
        Self {
            port,
            pending: VecDeque::new(),
            active_read: None,
            next_read_id: 1,
        }
    }
}

impl<P: SerialPort> CharacterDevice for SerialPortCharacterDevice<P> {
    fn prepare_read(&mut self, max_len: usize) -> DriverResult<Option<CharacterReadReservation>> {
        if max_len == 0 {
            return Ok(Some(CharacterReadReservation::new(0, Vec::new())));
        }
        if self.active_read.is_some() {
            return Ok(None);
        }
        let max_len = max_len.min(256);
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(max_len)
            .map_err(|_| DriverError::IoError)?;
        while bytes.len() < max_len {
            if let Some(byte) = self.pending.pop_front() {
                bytes.push(byte);
                continue;
            }
            let Some(byte) = self.port.try_read_byte() else {
                break;
            };
            bytes.push(byte);
        }
        if bytes.is_empty() {
            return Ok(None);
        }
        let id = self.next_read_id;
        self.next_read_id = self.next_read_id.wrapping_add(1);
        self.active_read = Some(id);
        Ok(Some(CharacterReadReservation::new(id, bytes)))
    }

    fn finish_read(
        &mut self,
        reservation: CharacterReadReservation,
        copied: usize,
        complete: bool,
    ) -> DriverResult<CharacterReadFinish> {
        let (id, bytes) = reservation.into_parts();
        if id == 0 && bytes.is_empty() && copied == 0 {
            return Ok(CharacterReadFinish::Bytes(0));
        }
        if self.active_read != Some(id) {
            return Err(DriverError::InvalidParam);
        }
        if copied > bytes.len() {
            for byte in bytes.into_iter().rev() {
                self.pending.push_front(byte);
            }
            self.active_read = None;
            return Err(DriverError::InvalidParam);
        }
        for &byte in bytes[copied..].iter().rev() {
            self.pending.push_front(byte);
        }
        self.active_read = None;
        if copied == 0 && !complete {
            Ok(CharacterReadFinish::Fault)
        } else {
            Ok(CharacterReadFinish::Bytes(copied))
        }
    }

    fn read(&mut self, buf: &mut [u8]) -> DriverResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let Some(reservation) = self.prepare_read(buf.len())? else {
            return Err(DriverError::Unsupported);
        };
        let len = reservation.bytes().len();
        buf[..len].copy_from_slice(reservation.bytes());
        match self.finish_read(reservation, len, true)? {
            CharacterReadFinish::Bytes(copied) => Ok(copied),
            CharacterReadFinish::Fault => Err(DriverError::IoError),
        }
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
        if events & POLLIN != 0 && self.active_read.is_none() {
            if self.pending.is_empty() {
                if let Some(byte) = self.port.try_read_byte() {
                    self.pending.push_back(byte);
                }
            }
            if !self.pending.is_empty() {
                revents |= POLLIN;
            }
        }
        if events & POLLOUT != 0 {
            revents |= POLLOUT;
        }
        Ok(revents)
    }
}

/// Verify that poll and read rollback never discard serial input.
pub fn test() {
    struct TestPort {
        input: VecDeque<u8>,
    }

    impl SerialPort for TestPort {
        fn write_byte(&mut self, _byte: u8) -> SerialResult<()> {
            Ok(())
        }

        fn read_byte_blocking(&mut self) -> u8 {
            self.input.pop_front().expect("test serial input")
        }

        fn try_read_byte(&mut self) -> Option<u8> {
            self.input.pop_front()
        }
    }

    let port = TestPort {
        input: VecDeque::from(Vec::from(*b"abc")),
    };
    let mut device = SerialPortCharacterDevice::new(port);
    assert_eq!(device.poll_revents(0x001), Ok(0x001));
    let reservation = device
        .prepare_read(3)
        .expect("prepare serial read")
        .expect("serial bytes");
    assert_eq!(reservation.bytes(), b"abc");
    assert_eq!(
        device.finish_read(reservation, 1, false),
        Ok(CharacterReadFinish::Bytes(1))
    );
    let reservation = device
        .prepare_read(2)
        .expect("prepare restored serial read")
        .expect("restored serial bytes");
    assert_eq!(reservation.bytes(), b"bc");
    assert_eq!(
        device.finish_read(reservation, 0, false),
        Ok(CharacterReadFinish::Fault)
    );
    let reservation = device
        .prepare_read(2)
        .expect("prepare cancelled serial read")
        .expect("cancelled serial bytes");
    assert_eq!(reservation.bytes(), b"bc");
    assert_eq!(
        device.finish_read(reservation, 2, true),
        Ok(CharacterReadFinish::Bytes(2))
    );
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
