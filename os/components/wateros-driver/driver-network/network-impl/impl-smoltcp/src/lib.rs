//! smoltcp 协议栈适配层：将 [`NetworkDevice`] 桥接到 smoltcp 的 [`Device`] trait。
//!
//! [`SmoltcpAdapter`] 持有内部缓冲区，在 token 生命期内完成帧的实际收发。

#![no_std]
extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use api_v0::{SharedNetworkDevice, DEFAULT_MTU};
use smoltcp::phy::{self, Device, DeviceCapabilities, Medium};
use smoltcp::time::Instant;

const RX_BUF: usize = 2048;
const TX_BUF: usize = 2048;
const MAX_LOOPBACK_FRAMES: usize = 4096;
const ETHERTYPE_IPV4: u16 = 0x0800;
const ETHERTYPE_ARP: u16 = 0x0806;

/// 将 [`SharedNetworkDevice`] 包装为 smoltcp 可用的网卡抽象。
pub struct SmoltcpAdapter {
    inner: Option<SharedNetworkDevice>,
    rx_buf: [u8; RX_BUF],
    tx_buf: [u8; TX_BUF],
    rx_len: usize,
    local_ipv4: [u8; 4],
    loopback_queue: VecDeque<Vec<u8>>,
}

/// smoltcp 要求的接收 token：指向已填充好数据的缓冲区切片。
pub struct SmoltcpRxToken<'a>(&'a [u8]);

/// smoltcp 要求的发送 token：持有发送缓冲区和对设备的引用。
pub struct SmoltcpTxToken<'a> {
    buf: &'a mut [u8],
    dev: Option<&'a SharedNetworkDevice>,
    local_ipv4: [u8; 4],
    loopback_queue: &'a mut VecDeque<Vec<u8>>,
}

impl SmoltcpAdapter {
    /// 用已注册的共享网络设备构造适配器。
    pub fn new(inner: SharedNetworkDevice) -> Self {
        Self::with_inner(Some(inner))
    }

    /// 构造只支持本机回环的适配器；用于无真实网卡时的 127.0.0.1。
    pub fn loopback_only() -> Self {
        Self::with_inner(None)
    }

    fn with_inner(inner: Option<SharedNetworkDevice>) -> Self {
        Self {
            inner,
            rx_buf: [0u8; RX_BUF],
            tx_buf: [0u8; TX_BUF],
            rx_len: 0,
            local_ipv4: [0; 4],
            loopback_queue: VecDeque::new(),
        }
    }

    /// 设置本机 IPv4 地址，用于识别应回灌给协议栈的本地帧。
    pub fn set_local_ipv4(&mut self, ip: [u8; 4]) {
        self.local_ipv4 = ip;
    }

    /// 回环队列中尚未被 `receive()` 取走的以太网帧数量。
    pub fn pending_loopback_frames(&self) -> usize {
        self.loopback_queue.len()
    }

    /// 获取 MAC 地址（用于构建 smoltcp 接口配置）。
    pub fn mac_address(&self) -> [u8; 6] {
        self.inner
            .as_ref()
            .map(|dev| dev.lock().mac_address())
            .unwrap_or([0x02, 0x00, 0x00, 0x00, 0x00, 0x01])
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
            local_ipv4,
            loopback_queue,
        } = self;

        if let Some(frame) = loopback_queue.pop_front() {
            let n = frame.len().min(RX_BUF);
            rx_buf[..n].copy_from_slice(&frame[..n]);
            *rx_len = n;
            return Some((
                SmoltcpRxToken(&rx_buf[..*rx_len]),
                SmoltcpTxToken {
                    buf: tx_buf,
                    dev: inner.as_ref(),
                    local_ipv4: *local_ipv4,
                    loopback_queue,
                },
            ));
        }

        if let Some(dev_handle) = inner.as_ref() {
            let mut dev = dev_handle.lock();
            match dev.receive(rx_buf) {
                Ok(n) if n > 0 => {
                    *rx_len = n;
                    drop(dev);
                    Some((
                        SmoltcpRxToken(&rx_buf[..*rx_len]),
                        SmoltcpTxToken {
                            buf: tx_buf,
                            dev: Some(dev_handle),
                            local_ipv4: *local_ipv4,
                            loopback_queue,
                        },
                    ))
                }
                _ => None,
            }
        } else {
            None
        }
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        let Self {
            inner,
            tx_buf,
            local_ipv4,
            loopback_queue,
            ..
        } = self;
        Some(SmoltcpTxToken {
            buf: tx_buf,
            dev: inner.as_ref(),
            local_ipv4: *local_ipv4,
            loopback_queue,
        })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = self
            .inner
            .as_ref()
            .map(|dev| dev.lock().mtu())
            .unwrap_or(DEFAULT_MTU);
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
        let frame = &self.buf[..len];
        if should_loopback(frame, self.local_ipv4) {
            if self.loopback_queue.len() < MAX_LOOPBACK_FRAMES {
                self.loopback_queue.push_back(frame.to_vec());
            } else {
                log::warn!("[smoltcp-adapter] loopback queue full, dropping local frame");
            }
            return result;
        }

        if let Some(dev) = self.dev {
            if let Err(e) = dev.lock().send(frame) {
                log::warn!("[smoltcp-adapter] tx send failed: {:?}", e);
            }
        }
        result
    }
}

fn should_loopback(frame: &[u8], local_ipv4: [u8; 4]) -> bool {
    if frame.len() < 14 || local_ipv4 == [0; 4] {
        return false;
    }

    let ethertype = u16::from_be_bytes([frame[12], frame[13]]);
    match ethertype {
        ETHERTYPE_IPV4 => {
            if frame.len() < 34 || frame[14] >> 4 != 4 {
                return false;
            }
            let ihl = usize::from(frame[14] & 0x0f) * 4;
            if ihl < 20 || frame.len() < 14 + ihl {
                return false;
            }
            let dst = [frame[30], frame[31], frame[32], frame[33]];
            is_local_ipv4(dst, local_ipv4)
        }
        ETHERTYPE_ARP => {
            if frame.len() < 42 {
                return false;
            }
            let htype = u16::from_be_bytes([frame[14], frame[15]]);
            let ptype = u16::from_be_bytes([frame[16], frame[17]]);
            let hlen = frame[18];
            let plen = frame[19];
            if htype != 1 || ptype != ETHERTYPE_IPV4 || hlen != 6 || plen != 4 {
                return false;
            }
            let target = [frame[38], frame[39], frame[40], frame[41]];
            is_local_ipv4(target, local_ipv4)
        }
        _ => false,
    }
}

fn is_local_ipv4(ip: [u8; 4], local_ipv4: [u8; 4]) -> bool {
    ip == local_ipv4 || ip[0] == 127
}
