//! smoltcp 协议栈适配层：将 [`NetworkDevice`] 桥接到 smoltcp 的 [`Device`] trait。
//!
//! [`SmoltcpAdapter`] 持有内部缓冲区，在 token 生命期内完成帧的实际收发。

#![no_std]
extern crate alloc;

use api_v0::SharedNetworkDevice;
use smoltcp::phy::{self, Device, DeviceCapabilities, Medium};
use smoltcp::time::Instant;

const RX_BUF: usize = 2048;
const TX_BUF: usize = 2048;

/// 将 [`SharedNetworkDevice`] 包装为 smoltcp 可用的网卡抽象。
pub struct SmoltcpAdapter {
    inner: SharedNetworkDevice,
    rx_buf: [u8; RX_BUF],
    tx_buf: [u8; TX_BUF],
    rx_len: usize,
}

/// smoltcp 要求的接收 token：指向已填充好数据的缓冲区切片。
pub struct SmoltcpRxToken<'a>(&'a [u8]);

/// smoltcp 要求的发送 token：持有发送缓冲区和对设备的引用。
pub struct SmoltcpTxToken<'a> {
    buf: &'a mut [u8],
    dev: &'a SharedNetworkDevice,
}

impl SmoltcpAdapter {
    /// 用已注册的共享网络设备构造适配器。
    pub fn new(inner: SharedNetworkDevice) -> Self {
        Self {
            inner,
            rx_buf: [0u8; RX_BUF],
            tx_buf: [0u8; TX_BUF],
            rx_len: 0,
        }
    }

    /// 获取 MAC 地址（用于构建 smoltcp 接口配置）。
    pub fn mac_address(&self) -> [u8; 6] {
        self.inner.lock().mac_address()
    }
}

impl Device for SmoltcpAdapter {
    type RxToken<'a> = SmoltcpRxToken<'a> where Self: 'a;
    type TxToken<'a> = SmoltcpTxToken<'a> where Self: 'a;

    fn receive(
        &mut self,
        _timestamp: Instant,
    ) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let Self {
            inner,
            rx_buf,
            tx_buf,
            rx_len,
        } = self;

        let mut dev = inner.lock();
        match dev.receive(rx_buf) {
            Ok(n) if n > 0 => {
                *rx_len = n;
                drop(dev);
                Some((
                    SmoltcpRxToken(&rx_buf[..*rx_len]),
                    SmoltcpTxToken {
                        buf: tx_buf,
                        dev: inner,
                    },
                ))
            }
            _ => None,
        }
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        let Self {
            inner, tx_buf, ..
        } = self;
        Some(SmoltcpTxToken {
            buf: tx_buf,
            dev: inner,
        })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = self.inner.lock().mtu();
        caps
    }
}

impl phy::RxToken for SmoltcpRxToken<'_> {
    fn consume<R, F: FnOnce(&[u8]) -> R>(self, f: F) -> R {
        f(self.0)
    }
}

impl phy::TxToken for SmoltcpTxToken<'_> {
    fn consume<R, F: FnOnce(&mut [u8]) -> R>(self, len: usize, f: F) -> R {
        let result = f(&mut self.buf[..len]);
        if let Err(e) = self.dev.lock().send(&self.buf[..len]) {
            log::warn!("[smoltcp-adapter] tx send failed: {:?}", e);
        }
        result
    }
}
