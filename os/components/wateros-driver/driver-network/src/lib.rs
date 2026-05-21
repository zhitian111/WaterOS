//! 网络设备子系统：DTB 绑定声明、网络 API 再导出，以及可选 NIC 实现。
//!
//! 扫描阶段仅依赖 [`NETWORK_SUPPORTED_DEVICES`] 与 [`supported_devices`]；具体网卡驱动在启用对应 feature 时提供。

#![no_std]
extern crate alloc;

use alloc::string::String;

pub use api_v0::*;
pub use driver_api::{DeviceType, SupportedDeviceEntry};

#[cfg(feature = "impl-dummy")]
#[doc(inline)]
pub use impl_dummy::DummyNetworkDevice;
#[cfg(feature = "impl-virtio-mmio")]
#[doc(inline)]
pub use impl_virtio_mmio::VirtioNetDevice;
#[cfg(feature = "impl-smoltcp")]
#[doc(inline)]
pub use impl_smoltcp::SmoltcpAdapter;

/// 网络子系统在 DTB 中声明可尝试绑定的设备（与 feature 无关；用于扫描阶段匹配）。
pub const NETWORK_SUPPORTED_DEVICES: &[SupportedDeviceEntry] = &[SupportedDeviceEntry {
    subsystem: "network",
    name: "virtio-net-mmio",
    compatible: "virtio,mmio",
}];

/// 返回本子系统声明支持的设备条目（非排他；可与其它子系统条目并存）。
pub fn supported_devices() -> &'static [SupportedDeviceEntry] {
    NETWORK_SUPPORTED_DEVICES
}

/// 网络子系统是否声明可处理该 DTB 设备（仅基于 `compatible` 列表与探测到的 [`DeviceType`]，不含具体初始化成败）。
pub fn network_subsystem_claims_device(compatibles: &[String], probed: DeviceType) -> bool {
    if probed != DeviceType::Network {
        return false;
    }
    supported_devices()
        .iter()
        .any(|s| s.subsystem == "network" && compatibles.iter().any(|c| c.as_str() == s.compatible))
}

/// 调用网络 API 自带自检（不访问真实硬件）。
pub fn test() {
    log::trace!("[driver-network] test begin");
    api_v0::test();
    log::trace!("[driver-network] test end");
}

#[cfg(feature = "impl-smoltcp")]
pub mod stack {
    //! smoltcp 协议栈初始化与轮询。
    //!
    //! 在设备驱动 `init_after_boot` 完成网卡注册后调用 [`init`]，
    //! 之后通过周期性 [`poll`] 驱动协议栈。

    use alloc::vec;
    use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
    use smoltcp::socket::{tcp, udp};
    use smoltcp::time::Instant;
    use smoltcp::wire::{EthernetAddress, HardwareAddress, IpCidr, Ipv4Address};
    use spin::Mutex;

    use crate::{first_network_device, SmoltcpAdapter};

    /// 协议栈全局状态：适配器 + 接口实例 + 套接字集合。
    pub struct NetworkStack {
        pub adapter: SmoltcpAdapter,
        pub iface: Interface,
        pub sockets: SocketSet<'static>,
        pub tcp_handle: SocketHandle,
        pub udp_handle: SocketHandle,
    }

    static NETWORK_STACK: Mutex<Option<NetworkStack>> = Mutex::new(None);

    /// 从全局注册表取出第一个网卡，创建 smoltcp 协议栈并配置 IP。
    ///
    /// # Panics
    /// 无已注册网卡时 panic。
    pub fn init(ip: [u8; 4], gateway: [u8; 4]) {
        let device =
            first_network_device().expect("[network-stack] no network device registered");
        let mac = device.lock().mac_address();
        let mut adapter = SmoltcpAdapter::new(device);

        let config = Config::new(HardwareAddress::Ethernet(EthernetAddress(mac)));
        let mut iface = Interface::new(config, &mut adapter, Instant::ZERO);

        iface.update_ip_addrs(|addrs| {
            addrs
                .push(IpCidr::new(
                    Ipv4Address::new(ip[0], ip[1], ip[2], ip[3]).into(),
                    24,
                ))
                .unwrap();
        });
        iface
            .routes_mut()
            .add_default_ipv4_route(Ipv4Address::new(
                gateway[0], gateway[1], gateway[2], gateway[3],
            ))
            .unwrap();

        let tcp_rx = tcp::SocketBuffer::new(vec![0; 4096]);
        let tcp_tx = tcp::SocketBuffer::new(vec![0; 4096]);
        let tcp_socket = tcp::Socket::new(tcp_rx, tcp_tx);

        let udp_rx = udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0; 2048]);
        let udp_tx = udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0; 2048]);
        let udp_socket = udp::Socket::new(udp_rx, udp_tx);

        let mut sockets = SocketSet::new(vec![]);
        let tcp_handle = sockets.add(tcp_socket);
        let udp_handle = sockets.add(udp_socket);

        *NETWORK_STACK.lock() = Some(NetworkStack {
            adapter,
            iface,
            sockets,
            tcp_handle,
            udp_handle,
        });

        log::info!(
            "[network-stack] initialized ip={}.{}.{}.{}/24 gateway={}.{}.{}.{}",
            ip[0], ip[1], ip[2], ip[3],
            gateway[0], gateway[1], gateway[2], gateway[3],
        );
    }

    /// 驱动协议栈处理一个轮询周期：收包 → 分发给 socket → 发送积压包。
    ///
    /// 需要在定时任务中周期性调用。
    pub fn poll() {
        let mut guard = NETWORK_STACK.lock();
        if let Some(stack) = guard.as_mut() {
            let NetworkStack {
                adapter,
                iface,
                sockets,
                ..
            } = stack;
            iface.poll(Instant::ZERO, adapter, sockets);
        }
    }

    /// 对 TCP socket 执行操作。返回 `None` 表示协议栈尚未初始化。
    pub fn with_tcp_socket<R>(f: impl FnOnce(&mut tcp::Socket) -> R) -> Option<R> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut()?;
        let NetworkStack {
            sockets, tcp_handle, ..
        } = stack;
        Some(f(sockets.get_mut::<tcp::Socket>(*tcp_handle)))
    }

    /// 对 UDP socket 执行操作。返回 `None` 表示协议栈尚未初始化。
    pub fn with_udp_socket<R>(f: impl FnOnce(&mut udp::Socket) -> R) -> Option<R> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut()?;
        let NetworkStack {
            sockets, udp_handle, ..
        } = stack;
        Some(f(sockets.get_mut::<udp::Socket>(*udp_handle)))
    }

    // ——— 便捷方法：隐藏 smoltcp 类型，供上层任务直接调用 ———

    /// TCP connect（仅排队，实际 SYN 由 poll 发出）。需要 interface context 做路由查询。
    pub fn tcp_connect(ip: [u8; 4], port: u16) -> Result<(), &'static str> {
        use smoltcp::wire::IpAddress;
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        let NetworkStack {
            iface,
            sockets,
            tcp_handle,
            ..
        } = stack;
        let cx = iface.context();
        sockets
            .get_mut::<tcp::Socket>(*tcp_handle)
            .connect(cx, (IpAddress::v4(ip[0], ip[1], ip[2], ip[3]), port), 0)
            .map_err(|_| "connect failed")
    }

    /// TCP 连接是否已建立（三次握手完成）。
    pub fn tcp_is_active() -> bool {
        with_tcp_socket(|s| s.is_active()).unwrap_or(false)
    }

    /// TCP 是否可发送数据。
    pub fn tcp_may_send() -> bool {
        with_tcp_socket(|s| s.may_send()).unwrap_or(false)
    }

    /// TCP 是否可接收数据。
    pub fn tcp_may_recv() -> bool {
        with_tcp_socket(|s| s.may_recv()).unwrap_or(false)
    }

    /// TCP 发送数据，返回已发送字节数。
    pub fn tcp_send(data: &[u8]) -> Result<usize, &'static str> {
        with_tcp_socket(|s| s.send_slice(data))
            .ok_or("stack not initialized")
            .and_then(|r| r.map_err(|_| "send failed"))
    }

    /// TCP 接收数据到缓冲区，返回已接收字节数。
    pub fn tcp_recv(buf: &mut [u8]) -> Result<usize, &'static str> {
        with_tcp_socket(|s| s.recv_slice(buf))
            .ok_or("stack not initialized")
            .and_then(|r| r.map_err(|_| "recv failed"))
    }

    /// UDP 发送数据到指定端点。
    pub fn udp_send(ip: [u8; 4], port: u16, data: &[u8]) -> Result<(), &'static str> {
        use smoltcp::wire::IpAddress;
        with_udp_socket(|s| {
            s.bind(0).ok();
            s.send_slice(data, (IpAddress::v4(ip[0], ip[1], ip[2], ip[3]), port))
        })
        .ok_or("stack not initialized")
        .and_then(|r| r.map_err(|_| "udp send failed"))
    }
}
