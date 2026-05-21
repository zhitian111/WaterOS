//! 网络设备占位实现：无硬件交互，用作缺省回退与编译占位。
//!
//! **当前行为**：无真实帧收发；**后续替换点**：virtio-net 等实现 crate。

#![no_std]
extern crate alloc;

use alloc::boxed::Box;
use alloc::sync::Arc;
use spin::Mutex;
use api_v0::{DriverError, DriverResult, NetworkDevice, SharedNetworkDevice};

/// 无操作网络设备：链路恒定 Down，收发均拒绝。
pub struct DummyNetworkDevice {
    mac: [u8; 6],
}

impl DummyNetworkDevice {
    /// 构造一个指定 MAC 的占位设备。
    pub fn new(mac: [u8; 6]) -> Self {
        Self { mac }
    }

    /// 创建一个已包装在 `Arc<Mutex<Box<dyn NetworkDevice>>>` 中的实例，便于直接注册。
    pub fn new_shared(mac: [u8; 6]) -> SharedNetworkDevice {
        Arc::new(Mutex::new(Box::new(Self::new(mac))))
    }
}

impl NetworkDevice for DummyNetworkDevice {
    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn is_link_up(&self) -> bool {
        false
    }

    fn send(&mut self, _buf: &[u8]) -> DriverResult<()> {
        Err(DriverError::Unsupported)
    }

    fn receive(&mut self, _buf: &mut [u8]) -> DriverResult<usize> {
        Err(DriverError::Unsupported)
    }
}
