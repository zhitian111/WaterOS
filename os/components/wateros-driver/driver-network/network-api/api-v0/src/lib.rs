//! 网络设备抽象：帧收发、MAC 地址与全局注册表。
//!
//! [`NetworkDevice`] 提供以太网帧的发送与接收接口；具体设备实现全局注册后由协议栈统一调度。

#![no_std]
extern crate alloc;

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

pub use driver_api::{DriverError, DriverResult};

/// 标准 IP MTU，即 Ethernet payload 上限；不包含 Ethernet 帧头与 FCS，具体设备可覆盖。
pub const DEFAULT_MTU: usize = 1500;

/// 可在多任务间共享的网络设备句柄（内部可变性由 `spin::Mutex` 提供）。
pub type SharedNetworkDevice = Arc<Mutex<Box<dyn NetworkDevice>>>;

struct RegisteredNetworkDevice {
    device: SharedNetworkDevice,
    present: Arc<AtomicBool>,
}

// slot 只追加、不复用；注销后保留空洞，旧索引不会指向另一块网卡。
static NETWORK_DEVICES: Mutex<Vec<Option<RegisteredNetworkDevice>>> = Mutex::new(Vec::new());

/// 协议栈持有的网卡租约。注销后共享设备对象可继续存活，但不得再执行 I/O。
#[derive(Clone)]
pub struct NetworkDeviceLease {
    index: usize,
    device: SharedNetworkDevice,
    present: Arc<AtomicBool>,
}

impl NetworkDeviceLease {
    pub fn index(&self) -> usize { self.index }
    pub fn device(&self) -> SharedNetworkDevice { self.device.clone() }
    pub fn is_present(&self) -> bool { self.present.load(Ordering::Acquire) }
}

/// 网络设备语义契约：收发以太网帧为必须实现的方法。
pub trait NetworkDevice: Send {
    /// 设备的 MAC 地址（6 字节）。
    fn mac_address(&self) -> [u8; 6];

    /// IP 最大传输单元（字节，不含 Ethernet 帧头与 FCS）；默认 [`DEFAULT_MTU`]。
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
    let index = devices.len();
    devices.push(Some(RegisteredNetworkDevice {
        device,
        present: Arc::new(AtomicBool::new(true)),
    }));
    drop(devices);
    driver_api::notify_device_topology_changed();
    index
}

/// 当前已注册网络设备数量。
pub fn network_device_count() -> usize {
    NETWORK_DEVICES.lock().iter().flatten().count()
}

/// 取表中第一个设备。
pub fn first_network_device() -> Option<SharedNetworkDevice> {
    NETWORK_DEVICES.lock().iter().flatten().next().map(|entry| entry.device.clone())
}

/// 按下标取设备；越界返回 `None`。
pub fn network_device_at(index: usize) -> Option<SharedNetworkDevice> {
    NETWORK_DEVICES.lock().get(index).and_then(Option::as_ref)
                   .map(|entry| entry.device.clone())
}

/// 返回第一个活动网卡及其可失效租约。
pub fn first_network_device_lease() -> Option<NetworkDeviceLease> {
    NETWORK_DEVICES.lock().iter().enumerate().find_map(|(index, slot)| {
        slot.as_ref().map(|entry| NetworkDeviceLease {
            index,
            device: entry.device.clone(),
            present: entry.present.clone(),
        })
    })
}

/// 按稳定 slot ID 获取可失效租约。
pub fn network_device_lease_at(index: usize) -> Option<NetworkDeviceLease> {
    NETWORK_DEVICES.lock().get(index).and_then(Option::as_ref).map(|entry| {
        NetworkDeviceLease {
            index,
            device: entry.device.clone(),
            present: entry.present.clone(),
        }
    })
}

/// 获取带稳定 slot ID 的活动网卡快照。
pub fn network_devices_snapshot() -> Vec<(usize, SharedNetworkDevice)> {
    NETWORK_DEVICES.lock().iter().enumerate().filter_map(|(index, slot)| {
        slot.as_ref().map(|entry| (index, entry.device.clone()))
    }).collect()
}

/// 注销网卡。真机驱动必须先屏蔽中断、停止 DMA 并完成所需 cache 同步。
///
/// 已安装协议栈的 lease 会立即失效，之后 adapter 不再调用旧设备的收发方法。
pub fn unregister_network_device(index: usize) -> bool {
    let mut devices = NETWORK_DEVICES.lock();
    let Some(slot) = devices.get_mut(index) else { return false };
    let Some(entry) = slot.take() else { return false };
    entry.present.store(false, Ordering::Release);
    drop(devices);
    driver_api::notify_device_topology_changed();
    true
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

#[cfg(test)]
mod tests {
    use super::*;

    fn shared_sample() -> SharedNetworkDevice {
        Arc::new(Mutex::new(Box::new(SampleNetworkDevice::new())))
    }

    #[test]
    fn unregister_invalidates_lease_and_preserves_stable_slots() {
        let generation = driver_api::device_topology_generation();
        let first = register_network_device(shared_sample());
        let lease = network_device_lease_at(first).expect("registered lease");
        assert!(lease.is_present());
        assert_eq!(lease.index(), first);
        assert!(driver_api::device_topology_generation() > generation);
        assert!(unregister_network_device(first));
        assert!(!lease.is_present());
        assert!(network_device_at(first).is_none());
        assert!(!unregister_network_device(first));

        let second = register_network_device(shared_sample());
        assert!(second > first);
        assert_eq!(network_devices_snapshot().last().unwrap().0, second);
        assert!(unregister_network_device(second));
    }
}
