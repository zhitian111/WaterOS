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
#[cfg(feature = "impl-virtio-pci")]
#[doc(inline)]
pub use impl_virtio_pci::{VirtioNetPciBarAllocator, VirtioNetPciProbeInfo, VirtioPciNetDevice};
#[cfg(feature = "impl-smoltcp")]
#[doc(inline)]
pub use impl_smoltcp::SmoltcpAdapter;
#[cfg(feature = "impl-smoltcp")]
pub mod socket_handles;
#[cfg(feature = "impl-smoltcp")]
pub use socket_handles::{SocketRef, TcpListenerHandle, TcpStreamHandle, UdpSocketHandle};

/// 网络子系统在 DTB 中声明可尝试绑定的设备（与 feature 无关；用于扫描阶段匹配）。
pub const NETWORK_SUPPORTED_DEVICES: &[SupportedDeviceEntry] = &[SupportedDeviceEntry {
    subsystem: "network",
    name: "virtio-net-mmio",
    compatible: "virtio,mmio",
}, SupportedDeviceEntry {
    subsystem: "network",
    name: "virtio-net-pci-transitional",
    compatible: "pci1af4,1000",
}, SupportedDeviceEntry {
    subsystem: "network",
    name: "virtio-net-pci-modern",
    compatible: "pci1af4,1041",
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

    use alloc::collections::BTreeMap;
    use alloc::vec;
    use alloc::vec::Vec;
    use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
    use smoltcp::socket::{tcp, udp};
    use smoltcp::time::Instant;
    use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr, IpListenEndpoint, Ipv4Address};
    use spin::Mutex;

    use crate::{first_network_device, SmoltcpAdapter};

    pub type StackSocketHandle = SocketHandle;

    /// Socket 状态机（内核侧跟踪，非 smoltcp 内部状态）。
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum SocketState {
        Created,
        Bound { port: u16 },
        Listening { port: u16 },
        Connecting,
        Connected,
        Closed,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum SocketKind {
        Tcp,
        Udp,
    }

    struct SocketMeta {
        kind: SocketKind,
        state: SocketState,
        /// None 表示绑定到 0.0.0.0，即任意本机地址。
        local_ip: Option<[u8; 4]>,
        /// TCP 监听 socket 标记：accept 后本 socket 变为已连接，需创建新监听器。
        is_listener: bool,
        /// 对端地址（connect 时填入，accept 后由上层填入）。
        peer_ip: [u8; 4],
        peer_port: u16,
    }

    /// 协议栈全局状态 + 动态 socket 管理。
    pub struct NetworkStack {
        adapter: SmoltcpAdapter,
        iface: Interface,
        sockets: SocketSet<'static>,
        metas: BTreeMap<SocketHandle, SocketMeta>,
        local_ip: [u8; 4],
        ephemeral_port: u16,
    }

    static NETWORK_STACK: Mutex<Option<NetworkStack>> = Mutex::new(None);

    /// 创建 smoltcp 协议栈并配置 IP；无真实网卡时仍启用 loopback-only 模式。
    pub fn init(ip: [u8; 4], gateway: [u8; 4]) -> Result<(), &'static str> {
        let mut adapter = match first_network_device() {
            Some(device) => SmoltcpAdapter::new(device),
            None => {
                log::warn!("[network-stack] no network device registered; using loopback-only mode");
                SmoltcpAdapter::loopback_only()
            }
        };
        let mac = adapter.mac_address();
        adapter.set_local_ipv4(ip);

        let config = Config::new(HardwareAddress::Ethernet(EthernetAddress(mac)));
        let mut iface = Interface::new(config, &mut adapter, Instant::ZERO);

        iface.update_ip_addrs(|addrs| {
            addrs
                .push(IpCidr::new(
                    Ipv4Address::new(ip[0], ip[1], ip[2], ip[3]).into(),
                    24,
                ))
                .unwrap();
            // loopback 地址：iperf / netperf / libc-test 均使用 127.0.0.1
            addrs
                .push(IpCidr::new(
                    Ipv4Address::new(127, 0, 0, 1).into(),
                    8,
                ))
                .unwrap();
        });
        // 默认路由：所有外部流量经网关
        iface
            .routes_mut()
            .add_default_ipv4_route(Ipv4Address::new(
                gateway[0], gateway[1], gateway[2], gateway[3],
            ))
            .unwrap();
        // 添加本地子网路由（直接可达，无需网关）和 loopback 路由
        iface.routes_mut().update(|storage| {
            // 本地子网 10.0.2.0/24 → 直接连接
            let _ = storage.push(smoltcp::iface::Route {
                cidr: smoltcp::wire::IpCidr::Ipv4(smoltcp::wire::Ipv4Cidr::new(
                    Ipv4Address::new(ip[0], ip[1], ip[2], ip[3]),
                    24,
                )),
                via_router: Ipv4Address::UNSPECIFIED.into(),
                preferred_until: None,
                expires_at: None,
            });
            // loopback 子网 127.0.0.0/8 → 本地
            let _ = storage.push(smoltcp::iface::Route {
                cidr: smoltcp::wire::IpCidr::Ipv4(smoltcp::wire::Ipv4Cidr::new(
                    Ipv4Address::new(127, 0, 0, 1),
                    8,
                )),
                via_router: Ipv4Address::UNSPECIFIED.into(),
                preferred_until: None,
                expires_at: None,
            });
        });

        *NETWORK_STACK.lock() = Some(NetworkStack {
            adapter,
            iface,
            sockets: SocketSet::new(vec![]),
            metas: BTreeMap::new(),
            local_ip: ip,
            ephemeral_port: 49152,
        });

        log::info!(
            "[network-stack] initialized ip={}.{}.{}.{}/24 gateway={}.{}.{}.{}",
            ip[0], ip[1], ip[2], ip[3],
            gateway[0], gateway[1], gateway[2], gateway[3],
        );
        Ok(())
    }

    /// 驱动协议栈处理一个轮询周期：收包 → 分发给 socket → 发送积压包。
    ///
    /// 需要在定时任务中周期性调用。
    pub fn poll() {
        poll_at_millis(0);
    }

    /// 使用调用方提供的单调毫秒时间驱动协议栈。
    pub fn poll_at_millis(millis: i64) {
        let mut guard = NETWORK_STACK.lock();
        if let Some(stack) = guard.as_mut() {
            let NetworkStack {
                adapter,
                iface,
                sockets,
                ..
            } = stack;
            iface.poll(Instant::from_millis(millis), adapter, sockets);
        }
    }

    /// 对 TCP socket 执行操作。返回 `None` 表示协议栈尚未初始化。
    pub fn with_tcp_socket<R>(handle: SocketHandle, f: impl FnOnce(&mut tcp::Socket) -> R) -> Option<R> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut()?;
        Some(f(stack.sockets.get_mut::<tcp::Socket>(handle)))
    }

    /// 对 UDP socket 执行操作。返回 `None` 表示协议栈尚未初始化。
    pub fn with_udp_socket<R>(handle: SocketHandle, f: impl FnOnce(&mut udp::Socket) -> R) -> Option<R> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut()?;
        Some(f(stack.sockets.get_mut::<udp::Socket>(handle)))
    }

    // ——— socket 工厂 ———

    /// 创建 TCP socket，返回其 smoltcp 句柄。
    pub fn create_tcp_socket() -> Result<SocketHandle, &'static str> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        let rx = tcp::SocketBuffer::new(vec![0; 4096]);
        let tx = tcp::SocketBuffer::new(vec![0; 4096]);
        let socket = tcp::Socket::new(rx, tx);
        let h = stack.sockets.add(socket);
        stack.metas.insert(h, SocketMeta {
            kind: SocketKind::Tcp,
            state: SocketState::Created,
            local_ip: None,
            is_listener: false,
            peer_ip: [0; 4],
            peer_port: 0,
        });
        Ok(h)
    }

    /// 创建 UDP socket，返回其 smoltcp 句柄。
    pub fn create_udp_socket() -> Result<SocketHandle, &'static str> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        let rx = udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0; 2048]);
        let tx = udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0; 2048]);
        let socket = udp::Socket::new(rx, tx);
        let h = stack.sockets.add(socket);
        stack.metas.insert(h, SocketMeta {
            kind: SocketKind::Udp,
            state: SocketState::Created,
            local_ip: None,
            is_listener: false,
            peer_ip: [0; 4],
            peer_port: 0,
        });
        Ok(h)
    }

    // ——— socket 操作 ———

    fn is_valid_local_addr(addr: Option<[u8; 4]>, configured: [u8; 4]) -> bool {
        match addr {
            None => true,
            Some(ip) => ip == configured || ip[0] == 127,
        }
    }

    fn listen_endpoint(addr: Option<[u8; 4]>, port: u16) -> IpListenEndpoint {
        IpListenEndpoint {
            addr: addr.map(|ip| IpAddress::v4(ip[0], ip[1], ip[2], ip[3])),
            port,
        }
    }

    /// 将 socket 绑定到本机地址/端口。None 表示 0.0.0.0 wildcard。
    /// TCP 仅记录本地端点；真正监听在 [`socket_listen`] 中执行。
    pub fn socket_bind(handle: SocketHandle, local_ip: Option<[u8; 4]>, port: u16) -> Result<(), &'static str> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        if !is_valid_local_addr(local_ip, stack.local_ip) {
            return Err("address not available");
        }
        let meta = stack.metas.get_mut(&handle).ok_or("invalid socket handle")?;
        match meta.kind {
            SocketKind::Tcp => {
                meta.state = SocketState::Bound { port };
                meta.local_ip = local_ip;
            }
            SocketKind::Udp => {
                stack.sockets
                    .get_mut::<udp::Socket>(handle)
                    .bind(listen_endpoint(local_ip, port))
                    .map_err(|_| "udp bind failed")?;
                meta.state = SocketState::Bound { port };
                meta.local_ip = local_ip;
            }
        }
        Ok(())
    }

    /// TCP socket 开始监听（需先 bind）。
    pub fn socket_listen(handle: SocketHandle) -> Result<(), &'static str> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        let meta = stack.metas.get_mut(&handle).ok_or("invalid socket handle")?;
        if meta.kind != SocketKind::Tcp {
            return Err("not a tcp socket");
        }
        let port = match meta.state {
            SocketState::Bound { port } => port,
            _ => return Err("socket not bound"),
        };
        let local_ip = meta.local_ip;
        stack.sockets
            .get_mut::<tcp::Socket>(handle)
            .listen(listen_endpoint(local_ip, port))
            .map_err(|_| "tcp listen failed")?;
        meta.state = SocketState::Listening { port };
        meta.is_listener = true;
        Ok(())
    }

    /// 获取 socket 的类型。
    pub fn socket_kind(handle: SocketHandle) -> Result<SocketKind, &'static str> {
        let guard = NETWORK_STACK.lock();
        let stack = guard.as_ref().ok_or("stack not initialized")?;
        stack.metas.get(&handle).map(|m| m.kind).ok_or("invalid socket handle")
    }

    /// 获取 socket 的状态。
    pub fn socket_state(handle: SocketHandle) -> Result<SocketState, &'static str> {
        let guard = NETWORK_STACK.lock();
        let stack = guard.as_ref().ok_or("stack not initialized")?;
        stack.metas.get(&handle).map(|m| m.state).ok_or("invalid socket handle")
    }

    /// 发起 TCP connect。非阻塞：返回后需 poll 驱动握手完成。
    pub fn socket_connect(handle: SocketHandle, ip: [u8; 4], port: u16) -> Result<(), &'static str> {
        use smoltcp::wire::IpAddress;
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        let meta = stack.metas.get_mut(&handle).ok_or("invalid socket handle")?;
        if meta.kind != SocketKind::Tcp {
            return Err("not a tcp socket");
        }
        let cx = stack.iface.context();
        // smoltcp 不接受 local_port=0，分配 ephemeral port
        let local_port = stack.ephemeral_port;
        stack.ephemeral_port = stack.ephemeral_port.wrapping_add(1);
        if stack.ephemeral_port == 0 {
            stack.ephemeral_port = 49152; // wrap back
        }
        stack.sockets
            .get_mut::<tcp::Socket>(handle)
            .connect(cx, (IpAddress::v4(ip[0], ip[1], ip[2], ip[3]), port), local_port)
            .map_err(|e| {
                log::warn!("[network-stack] connect err: {:?}, local_port={}", e, local_port);
                "connect failed"
            })?;
        meta.state = SocketState::Connecting;
        meta.peer_ip = ip;
        meta.peer_port = port;
        Ok(())
    }

    /// TCP connect 是否已建立。
    pub fn socket_is_connected(handle: SocketHandle) -> Result<bool, &'static str> {
        with_tcp_socket(handle, |s| s.is_active()).ok_or("stack not initialized")
    }

    /// TCP socket 是否可以发送。
    pub fn socket_may_send(handle: SocketHandle) -> Result<bool, &'static str> {
        with_tcp_socket(handle, |s| s.may_send()).ok_or("stack not initialized")
    }

    /// TCP socket 是否可以接收。
    pub fn socket_may_recv(handle: SocketHandle) -> Result<bool, &'static str> {
        with_tcp_socket(handle, |s| s.may_recv()).ok_or("stack not initialized")
    }

    /// 从 socket 发送数据（TCP 和已 connect 的 UDP）。
    pub fn socket_send(handle: SocketHandle, data: &[u8]) -> Result<usize, &'static str> {
        let guard = NETWORK_STACK.lock();
        let stack = guard.as_ref().ok_or("stack not initialized")?;
        let meta = stack.metas.get(&handle).ok_or("invalid socket handle")?;
        match meta.kind {
            SocketKind::Tcp => {
                drop(guard);
                with_tcp_socket(handle, |s| s.send_slice(data))
                    .ok_or("stack not initialized")
                    .and_then(|r| r.map_err(|_| "send failed"))
            }
            SocketKind::Udp => Err("udp: use sendto"),
        }
    }

    /// 从 socket 接收数据（TCP 和已 connect 的 UDP）。
    pub fn socket_recv(handle: SocketHandle, buf: &mut [u8]) -> Result<usize, &'static str> {
        let guard = NETWORK_STACK.lock();
        let stack = guard.as_ref().ok_or("stack not initialized")?;
        let meta = stack.metas.get(&handle).ok_or("invalid socket handle")?;
        match meta.kind {
            SocketKind::Tcp => {
                drop(guard);
                with_tcp_socket(handle, |s| s.recv_slice(buf))
                    .ok_or("stack not initialized")
                    .and_then(|r| r.map_err(|_| "recv failed"))
            }
            SocketKind::Udp => Err("udp: use recvfrom"),
        }
    }

    /// UDP sendto。
    pub fn socket_sendto(handle: SocketHandle, data: &[u8], ip: [u8; 4], port: u16) -> Result<usize, &'static str> {
        use smoltcp::wire::IpAddress;
        with_udp_socket(handle, |s| {
            s.send_slice(data, (IpAddress::v4(ip[0], ip[1], ip[2], ip[3]), port))
                .map(|()| data.len())
        })
        .ok_or("stack not initialized")
        .and_then(|r| r.map_err(|_| "udp sendto failed"))
    }

    /// UDP recvfrom。返回 (字节数, 来源IP, 来源端口)。
    pub fn socket_recvfrom(handle: SocketHandle, buf: &mut [u8]) -> Result<(usize, [u8; 4], u16), &'static str> {
        use smoltcp::wire::IpAddress;
        with_udp_socket(handle, |s| s.recv_slice(buf))
            .ok_or("stack not initialized")
            .and_then(|r| r.map_err(|_| "recvfrom failed"))
            .map(|(n, meta)| {
                let ip = match meta.endpoint.addr {
                    IpAddress::Ipv4(addr) => addr.octets(),
                };
                (n, ip, meta.endpoint.port)
            })
    }

    /// UDP socket 是否有数据可读。
    pub fn socket_udp_can_recv(handle: SocketHandle) -> Result<bool, &'static str> {
        with_udp_socket(handle, |s| s.can_recv()).ok_or("stack not initialized")
    }

    /// 设置 socket 选项（极简 stub：仅处理 SO_RCVTIMEO 等已知 option）。
    pub fn socket_setsockopt(_handle: SocketHandle, _level: usize, _optname: usize, _optval: &[u8]) -> Result<(), &'static str> {
        // TODO: 实际存储选项值
        Ok(())
    }

    /// 获取 socket 选项（极简 stub）。
    pub fn socket_getsockopt(_handle: SocketHandle, _level: usize, _optname: usize) -> Result<Vec<u8>, &'static str> {
        Ok(Vec::new())
    }

    /// 关闭 socket，从 SocketSet 中移除。
    pub fn socket_close(handle: SocketHandle) -> Result<(), &'static str> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        if stack.metas.remove(&handle).is_none() {
            return Err("invalid socket handle");
        }
        stack.sockets.remove(handle);
        Ok(())
    }

    /// 关闭 socket 的通信方向；当前 TCP 以全关闭近似实现，fd 仍由调用方保留。
    pub fn socket_shutdown(handle: SocketHandle) -> Result<(), &'static str> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        let meta = stack.metas.get_mut(&handle).ok_or("invalid socket handle")?;
        match meta.kind {
            SocketKind::Tcp => {
                stack.sockets.get_mut::<tcp::Socket>(handle).close();
                meta.state = SocketState::Closed;
                Ok(())
            }
            SocketKind::Udp => Err("shutdown unsupported for udp"),
        }
    }

    /// 检查 TCP 监听 socket 是否有入连接已完成握手。
    pub fn socket_has_pending_accept(handle: SocketHandle) -> Result<bool, &'static str> {
        let guard = NETWORK_STACK.lock();
        let stack = guard.as_ref().ok_or("stack not initialized")?;
        let meta = stack.metas.get(&handle).ok_or("invalid socket handle")?;
        if !meta.is_listener {
            return Err("not a listening socket");
        }
        drop(guard);
        with_tcp_socket(handle, |s| s.is_active()).ok_or("stack not initialized")
    }

    /// 接受 TCP 连接：原监听 socket 变为已连接 socket，并创建新的监听 socket 替换原 fd。
    /// 返回 (已建立连接的 socket_handle, 新监听 socket_handle, 本地端口)。
    pub fn socket_accept(handle: SocketHandle) -> Result<(SocketHandle, SocketHandle, u16), &'static str> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        let meta = stack.metas.get(&handle).ok_or("invalid socket handle")?;
        if !meta.is_listener {
            return Err("not a listening socket");
        }
        let port = match meta.state {
            SocketState::Listening { port } => port,
            _ => return Err("not listening"),
        };
        // 验证连接已建立
        {
            let tcp = stack.sockets.get_mut::<tcp::Socket>(handle);
            if !tcp.is_active() {
                return Err("no pending connection");
            }
        }
        // 原监听 socket → 已连接
        let meta = stack.metas.get_mut(&handle).unwrap();
        meta.state = SocketState::Connected;
        meta.is_listener = false;
        meta.peer_ip = [127, 0, 0, 1]; // loopback accept
        meta.peer_port = 0;
        // 创建替换监听 socket
        let rx = tcp::SocketBuffer::new(vec![0; 4096]);
        let tx = tcp::SocketBuffer::new(vec![0; 4096]);
        let mut new_listener = tcp::Socket::new(rx, tx);
        let local_ip = meta.local_ip;
        new_listener
            .listen(listen_endpoint(local_ip, port))
            .map_err(|_| "failed to create replacement listener")?;
        let new_h = stack.sockets.add(new_listener);
        stack.metas.insert(new_h, SocketMeta {
            kind: SocketKind::Tcp,
            state: SocketState::Listening { port },
            local_ip,
            is_listener: true,
            peer_ip: [0; 4],
            peer_port: 0,
        });
        Ok((handle, new_h, port))
    }

    /// 查询 socket 的对端地址（connect 或 accept 后有效）。
    pub fn socket_peername(handle: SocketHandle) -> Result<([u8; 4], u16), &'static str> {
        let guard = NETWORK_STACK.lock();
        let stack = guard.as_ref().ok_or("stack not initialized")?;
        let meta = stack.metas.get(&handle).ok_or("invalid socket handle")?;
        if meta.peer_ip == [0; 4] && meta.peer_port == 0 {
            return Err("not connected");
        }
        Ok((meta.peer_ip, meta.peer_port))
    }

    /// 查询 socket 绑定的本地端口。
    pub fn socket_local_port(handle: SocketHandle) -> Result<u16, &'static str> {
        let guard = NETWORK_STACK.lock();
        let stack = guard.as_ref().ok_or("stack not initialized")?;
        let meta = stack.metas.get(&handle).ok_or("invalid socket handle")?;
        match meta.state {
            SocketState::Bound { port } | SocketState::Listening { port } => Ok(port),
            _ => Err("socket not bound"),
        }
    }

    /// poll 后调用：更新所有 socket 的状态（Connecting → Connected）。
    pub fn poll_socket_events() {
        let mut guard = NETWORK_STACK.lock();
        let stack = match guard.as_mut() {
            Some(s) => s,
            None => return,
        };
        // 检查 Connecting → Connected 转换
        let mut updated: BTreeMap<SocketHandle, SocketState> = BTreeMap::new();
        for (&h, meta) in &stack.metas {
            if meta.state == SocketState::Connecting {
                if stack.sockets.get_mut::<tcp::Socket>(h).is_active() {
                    updated.insert(h, SocketState::Connected);
                }
            }
        }
        for (h, s) in updated {
            if let Some(meta) = stack.metas.get_mut(&h) {
                meta.state = s;
            }
        }
    }

    // ——— 兼容旧的便捷方法（供 self_tests/network.rs 使用） ———

    /// TCP connect（创建临时 socket → connect）。成功返回 socket handle。
    pub fn tcp_connect(ip: [u8; 4], port: u16) -> Result<SocketHandle, &'static str> {
        let h = create_tcp_socket()?;
        socket_connect(h, ip, port)?;
        Ok(h)
    }

    /// 检查最新创建的 TCP socket 是否激活（兼容旧 self_test）。
    pub fn tcp_is_active() -> bool {
        let mut guard = NETWORK_STACK.lock();
        let stack = match guard.as_mut() {
            Some(s) => s,
            None => return false,
        };
        stack.metas.iter().any(|(&h, m)| {
            m.kind == SocketKind::Tcp
                && m.state == SocketState::Connecting
                && stack.sockets.get_mut::<tcp::Socket>(h).is_active()
        })
    }

    /// TCP 是否可发送（遍历所有 TCP socket）。
    pub fn tcp_may_send() -> bool {
        let mut guard = NETWORK_STACK.lock();
        let stack = match guard.as_mut() {
            Some(s) => s,
            None => return false,
        };
        stack.metas.iter().any(|(&h, m)| {
            m.kind == SocketKind::Tcp && stack.sockets.get_mut::<tcp::Socket>(h).may_send()
        })
    }

    /// TCP 是否可接收（遍历所有 TCP socket）。
    pub fn tcp_may_recv() -> bool {
        let mut guard = NETWORK_STACK.lock();
        let stack = match guard.as_mut() {
            Some(s) => s,
            None => return false,
        };
        stack.metas.iter().any(|(&h, m)| {
            m.kind == SocketKind::Tcp && stack.sockets.get_mut::<tcp::Socket>(h).may_recv()
        })
    }

    /// TCP 发送（找到第一个已连接 TCP socket 发送）。
    pub fn tcp_send(data: &[u8]) -> Result<usize, &'static str> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        for (&h, m) in &stack.metas {
            if m.kind == SocketKind::Tcp && m.state == SocketState::Connected {
                return stack.sockets.get_mut::<tcp::Socket>(h)
                    .send_slice(data)
                    .map_err(|_| "send failed");
            }
        }
        Err("no connected tcp socket")
    }

    /// TCP 接收（从第一个已连接 TCP socket 接收）。
    pub fn tcp_recv(buf: &mut [u8]) -> Result<usize, &'static str> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        for (&h, m) in &stack.metas {
            if m.kind == SocketKind::Tcp && m.state == SocketState::Connected {
                return stack.sockets.get_mut::<tcp::Socket>(h)
                    .recv_slice(buf)
                    .map_err(|_| "recv failed");
            }
        }
        Err("no connected tcp socket")
    }

    /// UDP 发送（使用 UDP socket sendto）。
    pub fn udp_send(ip: [u8; 4], port: u16, data: &[u8]) -> Result<(), &'static str> {
        use smoltcp::wire::IpAddress;
        // 找第一个 UDP socket 或创建新的
        let guard = NETWORK_STACK.lock();
        let stack = guard.as_ref().ok_or("stack not initialized")?;
        let udp_handle = stack.metas.iter()
            .find(|(_, m)| m.kind == SocketKind::Udp)
            .map(|(&h, _)| h);
        drop(guard);
        let h = match udp_handle {
            Some(h) => h,
            None => create_udp_socket()?,
        };
        with_udp_socket(h, |s| {
            if let Err(e) = s.bind(0) {
                log::warn!("[network-stack] udp bind failed: {:?}", e);
            }
            s.send_slice(data, (IpAddress::v4(ip[0], ip[1], ip[2], ip[3]), port))
        })
        .ok_or("stack not initialized")
        .and_then(|r| r.map_err(|_| "udp send failed"))
    }
}
