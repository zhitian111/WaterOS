//! 网络设备抽象：帧收发、MAC 地址与全局注册表。
//!
//! [`NetworkDevice`] 提供以太网帧的发送与接收接口；具体设备实现全局注册后由协议栈统一调度。

#![no_std]
extern crate alloc;

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use spin::Mutex;

pub use driver_api::{DriverError, DriverResult};

/// 标准以太网 MTU（不含前导码/FCS）；具体设备可覆盖。
pub const DEFAULT_MTU: usize = 1500;

/// 可在多任务间共享的网络设备句柄（内部可变性由 `spin::Mutex` 提供）。
pub type SharedNetworkDevice = Arc<Mutex<Box<dyn NetworkDevice>>>;

// 注册顺序稳定：`register_network_device` 返回的下标即在此 `Vec` 中的位置。
static NETWORK_DEVICES: Mutex<Vec<SharedNetworkDevice>> = Mutex::new(Vec::new());

/// 网络设备语义契约：收发以太网帧为必须实现的方法。
pub trait NetworkDevice: Send {
    /// 设备的 MAC 地址（6 字节）。
    fn mac_address(&self) -> [u8; 6];

    /// 最大传输单元（字节）；默认 [`DEFAULT_MTU`]。
    fn mtu(&self) -> usize {
        DEFAULT_MTU
    }

    /// 链路是否就绪；默认 `true`，具体设备可按硬件状态覆盖。
    fn is_link_up(&self) -> bool {
        true
    }

    /// 发送一帧以太网数据；调用方负责构造完整的 L2 帧（含目的/源 MAC 与 EtherType）。
    fn send(&mut self, buf: &[u8]) -> DriverResult<()>;

    /// 接收一帧以太网数据；返回实际读入 `buf` 的字节数。
    /// 若 `buf` 长度不足容纳完整帧，应返回 [`DriverError::InvalidParam`]。
    fn receive(&mut self, buf: &mut [u8]) -> DriverResult<usize>;
}

/// 将设备追加到全局表末尾，返回其索引（从 0 起）。
pub fn register_network_device(device: SharedNetworkDevice) -> usize {
    let mut devices = NETWORK_DEVICES.lock();
    devices.push(device);
    devices.len() - 1
}

/// 当前已注册网络设备数量。
pub fn network_device_count() -> usize {
    NETWORK_DEVICES.lock().len()
}

/// 取表中第一个设备。
pub fn first_network_device() -> Option<SharedNetworkDevice> {
    NETWORK_DEVICES.lock().first().cloned()
}

/// 按下标取设备；越界返回 `None`。
pub fn network_device_at(index: usize) -> Option<SharedNetworkDevice> {
    NETWORK_DEVICES.lock().get(index).cloned()
}

/// 自检：校验常量与样例设备的收发行为。
pub fn test() {
    logging::trace!("[driver-network-api] test begin");
    assert_eq!(DEFAULT_MTU, 1500);
    let mut sample = SampleNetworkDevice::new();
    let mut buf = [0u8; DEFAULT_MTU];
    assert_eq!(sample.is_link_up(), true);
    sample.send(b"hello").expect("send should work");
    let n = sample.receive(&mut buf).expect("receive should work");
    assert_eq!(&buf[..n], b"hello");
    assert_eq!(sample.mac_address(), [0x02, 0x00, 0x00, 0x00, 0x00, 0x01]);
    logging::trace!("[driver-network-api] test end");
}

// 内存中的 Vec<u8> 模拟一个最简单的网络设备；send 写入内部缓冲区，receive 读出。
struct SampleNetworkDevice {
    mac: [u8; 6],
    buf: Vec<u8>,
}

impl SampleNetworkDevice {
    fn new() -> Self {
        Self {
            mac: [0x02, 0x00, 0x00, 0x00, 0x00, 0x01],
            buf: Vec::new(),
        }
    }
}

impl NetworkDevice for SampleNetworkDevice {
    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn send(&mut self, buf: &[u8]) -> DriverResult<()> {
        self.buf.extend_from_slice(buf);
        Ok(())
    }

    fn receive(&mut self, buf: &mut [u8]) -> DriverResult<usize> {
        if self.buf.is_empty() {
            return Ok(0);
        }
        let len = self.buf.len().min(buf.len());
        buf[..len].copy_from_slice(&self.buf[..len]);
        self.buf.drain(..len);
        Ok(len)
    }
}
