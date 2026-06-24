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

    use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
    use alloc::vec;
    use alloc::vec::Vec;
    use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
    use smoltcp::socket::{tcp, udp};
    use smoltcp::time::{Duration, Instant};
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
        local_port: u16,
        /// TCP 监听 socket 标记：accept 后本 socket 变为已连接，需创建新监听器。
        is_listener: bool,
        /// 对端地址（connect 时填入，accept 后由上层填入）。
        peer_ip: [u8; 4],
        peer_port: u16,
        /// SO_RCVTIMEO in milliseconds. None keeps the default blocking wait.
        recv_timeout_ms: Option<u64>,
        tcp_nodelay: bool,
        /// IPv4 组播成员（`MCAST_JOIN_GROUP` / `IP_ADD_MEMBERSHIP`）。
        mcast_groups: BTreeSet<u32>,
    }

    struct LoopbackUdpPacket {
        data: Vec<u8>,
        source_ip: [u8; 4],
        source_port: u16,
        dest_ip: [u8; 4],
    }

    /// 协议栈全局状态 + 动态 socket 管理。
    pub struct NetworkStack {
        adapter: SmoltcpAdapter,
        iface: Interface,
        sockets: SocketSet<'static>,
        metas: BTreeMap<SocketHandle, SocketMeta>,
        udp_loopback: BTreeMap<SocketHandle, VecDeque<LoopbackUdpPacket>>,
        udp_loopback_pending: BTreeMap<u16, VecDeque<LoopbackUdpPacket>>,
        local_ip: [u8; 4],
        ephemeral_port: u16,
    }

    static NETWORK_STACK: Mutex<Option<NetworkStack>> = Mutex::new(None);
    const TCP_BUFFER_SIZE: usize = 256 * 1024;
    const TCP_MSS: u32 = 1460;
    const UDP_LOOPBACK_QUEUE_LIMIT: usize = 1024;

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
            udp_loopback: BTreeMap::new(),
            udp_loopback_pending: BTreeMap::new(),
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
        let rx = tcp::SocketBuffer::new(vec![0; TCP_BUFFER_SIZE]);
        let tx = tcp::SocketBuffer::new(vec![0; TCP_BUFFER_SIZE]);
        let socket = tcp::Socket::new(rx, tx);
        let h = stack.sockets.add(socket);
        stack.metas.insert(h, SocketMeta {
            kind: SocketKind::Tcp,
            state: SocketState::Created,
            local_ip: None,
            local_port: 0,
            is_listener: false,
            peer_ip: [0; 4],
            peer_port: 0,
            recv_timeout_ms: None,
            tcp_nodelay: false,
            mcast_groups: BTreeSet::new(),
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
            local_port: 0,
            is_listener: false,
            peer_ip: [0; 4],
            peer_port: 0,
            recv_timeout_ms: None,
            tcp_nodelay: false,
            mcast_groups: BTreeSet::new(),
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

    fn next_ephemeral_port(stack: &mut NetworkStack) -> u16 {
        let port = stack.ephemeral_port;
        stack.ephemeral_port = stack.ephemeral_port.wrapping_add(1);
        if stack.ephemeral_port == 0 {
            stack.ephemeral_port = 49152;
        }
        port
    }

    fn is_local_destination(ip: [u8; 4], configured: [u8; 4]) -> bool {
        ip[0] == 127 || ip == configured
    }

    fn local_addr_matches(bound: Option<[u8; 4]>, dest: [u8; 4], configured: [u8; 4]) -> bool {
        match bound {
            None => true,
            Some(ip) if ip[0] == 127 && dest[0] == 127 => true,
            Some(ip) => ip == dest || (dest[0] == 127 && ip == configured),
        }
    }

    fn ensure_udp_bound(stack: &mut NetworkStack, handle: SocketHandle) -> Result<u16, &'static str> {
        let (kind, state, local_ip) = {
            let meta = stack.metas.get(&handle).ok_or("invalid socket handle")?;
            (meta.kind, meta.state, meta.local_ip)
        };
        if kind != SocketKind::Udp {
            return Err("not a udp socket");
        }
        match state {
            SocketState::Bound { port } => Ok(port),
            SocketState::Connected => {
                let port = stack
                    .metas
                    .get(&handle)
                    .ok_or("invalid socket handle")?
                    .local_port;
                if port == 0 {
                    Err("udp socket not bound")
                } else {
                    Ok(port)
                }
            }
            SocketState::Created => {
                let local_port = next_ephemeral_port(stack);
                stack
                    .sockets
                    .get_mut::<udp::Socket>(handle)
                    .bind(listen_endpoint(local_ip, local_port))
                    .map_err(|_| "udp bind failed")?;
                if let Some(meta) = stack.metas.get_mut(&handle) {
                    meta.state = SocketState::Bound { port: local_port };
                    meta.local_port = local_port;
                }
                Ok(local_port)
            }
            _ => Err("udp socket not bound"),
        }
    }

    fn queue_loopback_udp(
        stack: &mut NetworkStack,
        source_port: u16,
        dest_ip: [u8; 4],
        dest_port: u16,
        data: &[u8],
    ) -> bool {
        let source_ip = if dest_ip[0] == 127 {
            [127, 0, 0, 1]
        } else {
            stack.local_ip
        };
        let connected_target = stack.metas.iter().find_map(|(&h, meta)| {
            if meta.kind != SocketKind::Udp {
                return None;
            }
            match meta.state {
                SocketState::Connected
                    if meta.local_port == dest_port
                        && local_addr_matches(meta.local_ip, dest_ip, stack.local_ip)
                        && meta.peer_port == source_port
                        && (meta.peer_ip == source_ip
                            || (meta.peer_ip[0] == 127 && source_ip[0] == 127)) =>
                {
                    Some(h)
                }
                _ => None,
            }
        });
        let target = connected_target.or_else(|| {
            stack.metas.iter().find_map(|(&h, meta)| {
                if meta.kind != SocketKind::Udp {
                    return None;
                }
                match meta.state {
                    SocketState::Bound { port }
                        if port == dest_port && local_addr_matches(meta.local_ip, dest_ip, stack.local_ip) =>
                    {
                        Some(h)
                    }
                    _ => None,
                }
            })
        });
        let Some(target) = target else {
            let queue = stack
                .udp_loopback_pending
                .entry(dest_port)
                .or_insert_with(VecDeque::new);
            if queue.len() >= UDP_LOOPBACK_QUEUE_LIMIT {
                queue.pop_front();
            }
            queue.push_back(LoopbackUdpPacket {
                data: data.to_vec(),
                source_ip,
                source_port,
                dest_ip,
            });
            return true;
        };
        let queue = stack
            .udp_loopback
            .entry(target)
            .or_insert_with(VecDeque::new);
        if queue.len() >= UDP_LOOPBACK_QUEUE_LIMIT {
            queue.pop_front();
        }
        queue.push_back(LoopbackUdpPacket {
            data: data.to_vec(),
            source_ip,
            source_port,
            dest_ip,
        });
        true
    }

    fn udp_pending_matches(
        meta: &SocketMeta,
        packet: &LoopbackUdpPacket,
        local_ip: [u8; 4],
    ) -> bool {
        if !local_addr_matches(meta.local_ip, packet.dest_ip, local_ip) {
            return false;
        }
        match meta.state {
            SocketState::Bound { .. } => true,
            SocketState::Connected => {
                meta.peer_port == packet.source_port
                    && (meta.peer_ip == packet.source_ip
                        || (meta.peer_ip[0] == 127 && packet.source_ip[0] == 127))
            }
            _ => false,
        }
    }

    fn udp_local_port(meta: &SocketMeta) -> Option<u16> {
        match meta.state {
            SocketState::Bound { port } => Some(port),
            SocketState::Connected if meta.local_port != 0 => Some(meta.local_port),
            _ => None,
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
        // 先只读获取 socket 类型，避免后续与 next_ephemeral_port 的借用冲突
        let kind = stack.metas.get(&handle).ok_or("invalid socket handle")?.kind;
        match kind {
            SocketKind::Tcp => {
                // smoltcp 的 TCP listen 拒绝 port=0，且 getsockname 在 listen 之前
                // 就可能被调用（netperf 服务端流程：bind→getsockname→listen），
                // 因此必须在此处预分配 ephemeral port。
                let actual_port = if port == 0 {
                    next_ephemeral_port(stack)
                } else {
                    port
                };
                let meta = stack.metas.get_mut(&handle).ok_or("invalid socket handle")?;
                meta.state = SocketState::Bound { port: actual_port };
                meta.local_ip = local_ip;
                meta.local_port = actual_port;
            }
            SocketKind::Udp => {
                // smoltcp 的 UDP bind 拒绝 port=0，必须预分配 ephemeral port
                let actual_port = if port == 0 {
                    next_ephemeral_port(stack)
                } else {
                    port
                };
                stack.sockets
                    .get_mut::<udp::Socket>(handle)
                    .bind(listen_endpoint(local_ip, actual_port))
                    .map_err(|_| "udp bind failed")?;
                let meta = stack.metas.get_mut(&handle).ok_or("invalid socket handle")?;
                meta.state = SocketState::Bound { port: actual_port };
                meta.local_ip = local_ip;
                meta.local_port = actual_port;
            }
        }
        Ok(())
    }

    /// TCP socket 开始监听（需先 bind）。
    pub fn socket_listen(handle: SocketHandle) -> Result<(), &'static str> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        // 先只读提取端口和本地 IP
        let (mut port, local_ip) = {
            let meta = stack.metas.get(&handle).ok_or("invalid socket handle")?;
            if meta.kind != SocketKind::Tcp {
                return Err("not a tcp socket");
            }
            let port = match meta.state {
                SocketState::Bound { port } => port,
                _ => return Err("socket not bound"),
            };
            (port, meta.local_ip)
        };
        // 若 bind 时指定 port=0，自动分配 ephemeral port
        if port == 0 {
            port = next_ephemeral_port(stack);
            let meta = stack.metas.get_mut(&handle).ok_or("invalid socket handle")?;
            meta.state = SocketState::Bound { port };
            meta.local_port = port;
        }
        stack.sockets
            .get_mut::<tcp::Socket>(handle)
            .listen(listen_endpoint(local_ip, port))
            .map_err(|_| "tcp listen failed")?;
        let meta = stack.metas.get_mut(&handle).ok_or("invalid socket handle")?;
        meta.state = SocketState::Listening { port };
        meta.local_port = port;
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

    /// 发起 TCP/UDP connect。TCP 非阻塞返回后需 poll 驱动握手完成；UDP 只记录默认 peer。
    pub fn socket_connect(handle: SocketHandle, ip: [u8; 4], port: u16) -> Result<(), &'static str> {
        use smoltcp::wire::IpAddress;
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        let (kind, state) = {
            let meta = stack.metas.get(&handle).ok_or("invalid socket handle")?;
            (meta.kind, meta.state)
        };
        match kind {
            SocketKind::Tcp => {
                // smoltcp 不接受 local_port=0，分配 ephemeral port
                let local_port = next_ephemeral_port(stack);
                let cx = stack.iface.context();
                stack.sockets
                    .get_mut::<tcp::Socket>(handle)
                    .connect(cx, (IpAddress::v4(ip[0], ip[1], ip[2], ip[3]), port), local_port)
                    .map_err(|e| {
                        log::warn!("[network-stack] connect err: {:?}, local_port={}", e, local_port);
                        "connect failed"
                    })?;
                if let Some(meta) = stack.metas.get_mut(&handle) {
                    meta.state = SocketState::Connecting;
                    meta.local_port = local_port;
                }
            }
            SocketKind::Udp => {
                if matches!(state, SocketState::Created) {
                    ensure_udp_bound(stack, handle)?;
                }
                if let Some(meta) = stack.metas.get_mut(&handle) {
                    meta.state = SocketState::Connected;
                }
            }
        }
        let meta = stack.metas.get_mut(&handle).ok_or("invalid socket handle")?;
        meta.peer_ip = ip;
        meta.peer_port = port;
        Ok(())
    }

    /// 重新发起 TCP connect，供阻塞式 connect 在早期 RST/监听端尚未就绪时重试。
    pub fn socket_retry_connect(handle: SocketHandle) -> Result<(), &'static str> {
        use smoltcp::wire::IpAddress;
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        let (kind, ip, port) = {
            let meta = stack.metas.get(&handle).ok_or("invalid socket handle")?;
            (meta.kind, meta.peer_ip, meta.peer_port)
        };
        if kind != SocketKind::Tcp {
            return Err("not a tcp socket");
        }
        if ip == [0; 4] && port == 0 {
            return Err("tcp peer not set");
        }
        let local_port = next_ephemeral_port(stack);
        let cx = stack.iface.context();
        let socket = stack.sockets.get_mut::<tcp::Socket>(handle);
        socket.abort();
        socket
            .connect(cx, (IpAddress::v4(ip[0], ip[1], ip[2], ip[3]), port), local_port)
            .map_err(|_| "connect failed")?;
        if let Some(meta) = stack.metas.get_mut(&handle) {
            meta.state = SocketState::Connecting;
            meta.local_port = local_port;
        }
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

    /// TCP socket 当前发送缓冲还能容纳的字节数。
    pub fn socket_send_capacity(handle: SocketHandle) -> Result<usize, &'static str> {
        with_tcp_socket(handle, |s| s.send_capacity()).ok_or("stack not initialized")
    }

    /// TCP socket 是否可以接收。
    pub fn socket_may_recv(handle: SocketHandle) -> Result<bool, &'static str> {
        with_tcp_socket(handle, |s| s.may_recv()).ok_or("stack not initialized")
    }

    /// TCP socket 当前是否有数据可读。
    pub fn socket_can_recv(handle: SocketHandle) -> Result<bool, &'static str> {
        with_tcp_socket(handle, |s| s.can_recv()).ok_or("stack not initialized")
    }

    /// 从 socket 发送数据（TCP 和已 connect 的 UDP）。
    pub fn socket_send(handle: SocketHandle, data: &[u8]) -> Result<usize, &'static str> {
        let guard = NETWORK_STACK.lock();
        let stack = guard.as_ref().ok_or("stack not initialized")?;
        let meta = stack.metas.get(&handle).ok_or("invalid socket handle")?;
        match meta.kind {
            SocketKind::Tcp => {
                drop(guard);
                let sent = with_tcp_socket(handle, |s| s.send_slice(data))
                    .ok_or("stack not initialized")
                    .and_then(|r| r.map_err(|_| "send failed"));
                if sent.is_ok() {
                    poll();
                    poll_socket_events();
                }
                sent
            }
            SocketKind::Udp => {
                let ip = meta.peer_ip;
                let port = meta.peer_port;
                drop(guard);
                if ip == [0; 4] && port == 0 {
                    return Err("udp not connected");
                }
                socket_sendto(handle, data, ip, port)
            }
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
                let received = with_tcp_socket(handle, |s| s.recv_slice(buf))
                    .ok_or("stack not initialized")
                    .and_then(|r| r.map_err(|_| "recv failed"));
                if received.is_ok() {
                    poll();
                    poll_socket_events();
                }
                received
            }
            SocketKind::Udp => {
                drop(guard);
                socket_recvfrom(handle, buf).map(|(n, _, _)| n)
            }
        }
    }

    /// UDP sendto。
    pub fn socket_sendto(handle: SocketHandle, data: &[u8], ip: [u8; 4], port: u16) -> Result<usize, &'static str> {
        use smoltcp::wire::IpAddress;
        {
            let mut guard = NETWORK_STACK.lock();
            let stack = guard.as_mut().ok_or("stack not initialized")?;
            let source_port = ensure_udp_bound(stack, handle)?;
            if is_local_destination(ip, stack.local_ip)
                && queue_loopback_udp(stack, source_port, ip, port, data)
            {
                return Ok(data.len());
            }
        }
        let sent = with_udp_socket(handle, |s| {
            s.send_slice(data, (IpAddress::v4(ip[0], ip[1], ip[2], ip[3]), port))
                .map(|()| data.len())
        })
        .ok_or("stack not initialized")
        .and_then(|r| r.map_err(|_| "udp sendto failed"));
        if sent.is_ok() {
            poll();
            poll_socket_events();
        }
        sent
    }

    /// UDP recvfrom。返回 (字节数, 来源IP, 来源端口)。
    pub fn socket_recvfrom(handle: SocketHandle, buf: &mut [u8]) -> Result<(usize, [u8; 4], u16), &'static str> {
        use smoltcp::wire::IpAddress;
        {
            let mut guard = NETWORK_STACK.lock();
            let stack = guard.as_mut().ok_or("stack not initialized")?;
            if let Some(queue) = stack.udp_loopback.get_mut(&handle) {
                if let Some(packet) = queue.pop_front() {
                    let n = packet.data.len().min(buf.len());
                    buf[..n].copy_from_slice(&packet.data[..n]);
                    return Ok((n, packet.source_ip, packet.source_port));
                }
            }
            if let Some(meta) = stack.metas.get(&handle) {
                if let Some(port) = udp_local_port(meta) {
                    if let Some(queue) = stack.udp_loopback_pending.get_mut(&port) {
                        if let Some(index) = queue
                            .iter()
                            .position(|packet| udp_pending_matches(meta, packet, stack.local_ip))
                        {
                            if let Some(packet) = queue.remove(index) {
                                let n = packet.data.len().min(buf.len());
                                buf[..n].copy_from_slice(&packet.data[..n]);
                                return Ok((n, packet.source_ip, packet.source_port));
                            }
                        }
                    }
                }
            }
        }
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
        {
            let guard = NETWORK_STACK.lock();
            let stack = guard.as_ref().ok_or("stack not initialized")?;
            if stack
                .udp_loopback
                .get(&handle)
                .is_some_and(|queue| !queue.is_empty())
            {
                return Ok(true);
            }
            if let Some(meta) = stack.metas.get(&handle) {
                if let Some(port) = udp_local_port(meta) {
                    if stack
                        .udp_loopback_pending
                        .get(&port)
                        .is_some_and(|queue| {
                            queue
                                .iter()
                                .any(|packet| udp_pending_matches(meta, packet, stack.local_ip))
                        })
                    {
                        return Ok(true);
                    }
                }
            }
        }
        with_udp_socket(handle, |s| s.can_recv()).ok_or("stack not initialized")
    }

    fn timeval_to_millis(optval: &[u8]) -> Result<Option<u64>, &'static str> {
        if optval.len() >= 16 {
            let mut sec = [0u8; 8];
            let mut usec = [0u8; 8];
            sec.copy_from_slice(&optval[0..8]);
            usec.copy_from_slice(&optval[8..16]);
            let sec = i64::from_ne_bytes(sec);
            let usec = i64::from_ne_bytes(usec);
            if sec < 0 || usec < 0 || usec >= 1_000_000 {
                return Err("invalid timeval");
            }
            if sec == 0 && usec == 0 {
                return Ok(None);
            }
            let millis = (sec as u64)
                .saturating_mul(1000)
                .saturating_add(((usec as u64).saturating_add(999)) / 1000)
                .max(1);
            return Ok(Some(millis));
        }
        if optval.len() >= 8 {
            let mut sec = [0u8; 4];
            let mut usec = [0u8; 4];
            sec.copy_from_slice(&optval[0..4]);
            usec.copy_from_slice(&optval[4..8]);
            let sec = i32::from_ne_bytes(sec);
            let usec = i32::from_ne_bytes(usec);
            if sec < 0 || usec < 0 || usec >= 1_000_000 {
                return Err("invalid timeval");
            }
            if sec == 0 && usec == 0 {
                return Ok(None);
            }
            let millis = (sec as u64)
                .saturating_mul(1000)
                .saturating_add(((usec as u64).saturating_add(999)) / 1000)
                .max(1);
            return Ok(Some(millis));
        }
        Err("invalid timeval")
    }

    fn millis_to_timeval(timeout_ms: Option<u64>) -> Vec<u8> {
        let millis = timeout_ms.unwrap_or(0);
        let sec = (millis / 1000) as i64;
        let usec = ((millis % 1000) * 1000) as i64;
        let mut out = Vec::with_capacity(16);
        out.extend_from_slice(&sec.to_ne_bytes());
        out.extend_from_slice(&usec.to_ne_bytes());
        out
    }

    fn sockopt_bool(optval: &[u8]) -> Result<bool, &'static str> {
        if optval.is_empty() {
            return Err("invalid bool sockopt");
        }
        if optval.len() >= 4 {
            let mut raw = [0u8; 4];
            raw.copy_from_slice(&optval[..4]);
            return Ok(i32::from_ne_bytes(raw) != 0);
        }
        Ok(optval.iter().any(|&b| b != 0))
    }

    fn sockopt_i32(optval: &[u8]) -> Result<i32, &'static str> {
        if optval.len() < 4 {
            return Err("invalid int sockopt");
        }
        let mut raw = [0u8; 4];
        raw.copy_from_slice(&optval[..4]);
        Ok(i32::from_ne_bytes(raw))
    }

    fn parse_ipv4_mcast_group(optval: &[u8]) -> Result<u32, &'static str> {
        if optval.len() >= 12 {
            let family = u16::from_ne_bytes([optval[4], optval[5]]);
            if family == 2 {
                return Ok(u32::from_ne_bytes([optval[8], optval[9], optval[10], optval[11]]));
            }
        }
        if optval.len() >= 8 {
            return Ok(u32::from_ne_bytes([optval[0], optval[1], optval[2], optval[3]]));
        }
        Err("invalid multicast request")
    }

    fn mcast_join(meta: &mut SocketMeta, group: u32) {
        meta.mcast_groups.insert(group);
    }

    fn mcast_leave(meta: &mut SocketMeta, group: u32) -> Result<(), &'static str> {
        if meta.mcast_groups.remove(&group) {
            Ok(())
        } else {
            Err("addr not available")
        }
    }

    /// 设置 socket 选项（支持常见 iperf 依赖的 SOL_SOCKET timeout/buffer 选项）。
    pub fn socket_setsockopt(handle: SocketHandle, level: usize, optname: usize, optval: &[u8]) -> Result<(), &'static str> {
        const SOL_SOCKET: usize = 1;
        const SOL_IP: usize = 0;
        const IPPROTO_IP: usize = 0;
        const SO_REUSEADDR: usize = 2;
        const SO_REUSEPORT: usize = 15;
        const SO_SNDBUF: usize = 7;
        const SO_RCVBUF: usize = 8;
        const SO_RCVTIMEO_OLD: usize = 20;
        const SO_SNDTIMEO_OLD: usize = 21;
        const SO_RCVTIMEO_NEW: usize = 66;
        const SO_SNDTIMEO_NEW: usize = 67;
        const IPPROTO_TCP: usize = 6;
        const TCP_NODELAY: usize = 1;
        const IP_ADD_MEMBERSHIP: usize = 35;
        const IP_DROP_MEMBERSHIP: usize = 36;
        const MCAST_JOIN_GROUP: usize = 42;
        const MCAST_LEAVE_GROUP: usize = 45;

        if (level == SOL_IP || level == IPPROTO_IP)
            && matches!(optname, IP_ADD_MEMBERSHIP | MCAST_JOIN_GROUP)
        {
            let group = parse_ipv4_mcast_group(optval)?;
            let mut guard = NETWORK_STACK.lock();
            let stack = guard.as_mut().ok_or("stack not initialized")?;
            let meta = stack.metas.get_mut(&handle).ok_or("invalid socket handle")?;
            mcast_join(meta, group);
            return Ok(());
        }
        if (level == SOL_IP || level == IPPROTO_IP)
            && matches!(optname, IP_DROP_MEMBERSHIP | MCAST_LEAVE_GROUP)
        {
            let group = parse_ipv4_mcast_group(optval)?;
            let mut guard = NETWORK_STACK.lock();
            let stack = guard.as_mut().ok_or("stack not initialized")?;
            let meta = stack.metas.get_mut(&handle).ok_or("invalid socket handle")?;
            return mcast_leave(meta, group);
        }

        if level == SOL_SOCKET && matches!(optname, SO_REUSEADDR | SO_REUSEPORT) {
            return Ok(());
        }
        // netperf/iperf 会 setsockopt(SO_SNDBUF/SO_RCVBUF)；smoltcp 缓冲固定，接受请求即可。
        if level == SOL_SOCKET && (optname == SO_SNDBUF || optname == SO_RCVBUF) {
            let _ = sockopt_i32(optval)?;
            return Ok(());
        }
        if level == SOL_SOCKET && (optname == SO_RCVTIMEO_OLD || optname == SO_RCVTIMEO_NEW) {
            let timeout_ms = timeval_to_millis(optval)?;
            let mut guard = NETWORK_STACK.lock();
            let stack = guard.as_mut().ok_or("stack not initialized")?;
            let meta = stack.metas.get_mut(&handle).ok_or("invalid socket handle")?;
            meta.recv_timeout_ms = timeout_ms;
            return Ok(());
        }
        if level == SOL_SOCKET && (optname == SO_SNDTIMEO_OLD || optname == SO_SNDTIMEO_NEW) {
            let _ = timeval_to_millis(optval)?;
            return Ok(());
        }
        if level == IPPROTO_TCP && optname == TCP_NODELAY {
            let enabled = sockopt_bool(optval)?;
            let mut guard = NETWORK_STACK.lock();
            let stack = guard.as_mut().ok_or("stack not initialized")?;
            let kind = stack
                .metas
                .get(&handle)
                .map(|meta| meta.kind)
                .ok_or("invalid socket handle")?;
            if kind != SocketKind::Tcp {
                return Err("not a tcp socket");
            }
            let socket = stack.sockets.get_mut::<tcp::Socket>(handle);
            socket.set_nagle_enabled(!enabled);
            socket.set_ack_delay(if enabled {
                None
            } else {
                Some(Duration::from_millis(10))
            });
            let meta = stack.metas.get_mut(&handle).ok_or("invalid socket handle")?;
            meta.tcp_nodelay = enabled;
            drop(guard);
            poll_socket_events();
            return Ok(());
        }
        Err("unsupported sockopt")
    }

    /// 查询 SO_RCVTIMEO，供 syscall 阻塞接收路径换算等待 tick。
    pub fn socket_recv_timeout_ms(handle: SocketHandle) -> Result<Option<u64>, &'static str> {
        let guard = NETWORK_STACK.lock();
        let stack = guard.as_ref().ok_or("stack not initialized")?;
        let meta = stack.metas.get(&handle).ok_or("invalid socket handle")?;
        Ok(meta.recv_timeout_ms)
    }

    fn write_u32(buf: &mut [u8], offset: usize, value: u32) {
        if offset + 4 <= buf.len() {
            buf[offset..offset + 4].copy_from_slice(&value.to_ne_bytes());
        }
    }

    fn tcp_info(handle: SocketHandle) -> Vec<u8> {
        const TCP_INFO_LEN: usize = 256;
        const TCP_ESTABLISHED: u8 = 1;
        const TCP_CLOSE: u8 = 7;

        let mut out = vec![0u8; TCP_INFO_LEN];
        let connected = socket_is_connected(handle).unwrap_or(false);
        out[0] = if connected { TCP_ESTABLISHED } else { TCP_CLOSE };

        let send_capacity = socket_send_capacity(handle).unwrap_or(0) as u32;
        let cwnd_segments = (send_capacity / TCP_MSS).clamp(2, 64);

        // Linux uapi struct tcp_info offsets used by iperf3.
        write_u32(&mut out, 8, 200_000); // tcpi_rto, usec
        write_u32(&mut out, 16, TCP_MSS); // tcpi_snd_mss
        write_u32(&mut out, 20, TCP_MSS); // tcpi_rcv_mss
        write_u32(&mut out, 60, 1500); // tcpi_pmtu
        write_u32(&mut out, 64, TCP_BUFFER_SIZE as u32); // tcpi_rcv_ssthresh
        write_u32(&mut out, 68, 1_000); // tcpi_rtt, usec
        write_u32(&mut out, 72, 250); // tcpi_rttvar, usec
        write_u32(&mut out, 76, u32::MAX); // tcpi_snd_ssthresh
        write_u32(&mut out, 80, cwnd_segments); // tcpi_snd_cwnd, packets
        write_u32(&mut out, 84, TCP_MSS); // tcpi_advmss
        write_u32(&mut out, 88, 3); // tcpi_reordering
        write_u32(&mut out, 96, TCP_BUFFER_SIZE as u32); // tcpi_rcv_space
        write_u32(&mut out, 100, 0); // tcpi_total_retrans
        write_u32(&mut out, 228, TCP_BUFFER_SIZE as u32); // tcpi_snd_wnd on newer Linux
        out
    }

    /// 获取 socket 选项（极简 stub）。
    pub fn socket_getsockopt(handle: SocketHandle, level: usize, optname: usize) -> Result<Vec<u8>, &'static str> {
        const SOL_SOCKET: usize = 1;
        const SO_ERROR: usize = 4;
        const SO_SNDBUF: usize = 7;
        const SO_RCVBUF: usize = 8;
        const SO_RCVTIMEO_OLD: usize = 20;
        const SO_SNDTIMEO_OLD: usize = 21;
        const SO_RCVTIMEO_NEW: usize = 66;
        const SO_SNDTIMEO_NEW: usize = 67;
        const IPPROTO_TCP: usize = 6;
        const TCP_NODELAY: usize = 1;
        const TCP_MAXSEG: usize = 2;
        const TCP_INFO: usize = 11;

        if level == SOL_SOCKET && optname == SO_ERROR {
            return Ok(0i32.to_ne_bytes().to_vec());
        }
        if level == SOL_SOCKET && (optname == SO_SNDBUF || optname == SO_RCVBUF) {
            return Ok((TCP_BUFFER_SIZE as i32).to_ne_bytes().to_vec());
        }
        if level == SOL_SOCKET && (optname == SO_RCVTIMEO_OLD || optname == SO_RCVTIMEO_NEW) {
            let timeout = {
                let guard = NETWORK_STACK.lock();
                let stack = guard.as_ref().ok_or("stack not initialized")?;
                let meta = stack.metas.get(&handle).ok_or("invalid socket handle")?;
                meta.recv_timeout_ms
            };
            return Ok(millis_to_timeval(timeout));
        }
        if level == SOL_SOCKET && (optname == SO_SNDTIMEO_OLD || optname == SO_SNDTIMEO_NEW) {
            return Ok(millis_to_timeval(None));
        }
        if level == IPPROTO_TCP && optname == TCP_NODELAY {
            let enabled = {
                let guard = NETWORK_STACK.lock();
                let stack = guard.as_ref().ok_or("stack not initialized")?;
                let meta = stack.metas.get(&handle).ok_or("invalid socket handle")?;
                meta.tcp_nodelay
            };
            return Ok((enabled as i32).to_ne_bytes().to_vec());
        }
        if level == IPPROTO_TCP && optname == TCP_MAXSEG {
            return Ok((TCP_MSS as i32).to_ne_bytes().to_vec());
        }
        if level == IPPROTO_TCP && optname == TCP_INFO {
            return Ok(tcp_info(handle));
        }
        Ok(Vec::new())
    }

    /// 关闭 socket，从 SocketSet 中移除。
    pub fn socket_close(handle: SocketHandle) -> Result<(), &'static str> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        let meta = stack.metas.get_mut(&handle).ok_or("invalid socket handle")?;
        let should_poll = match (meta.kind, meta.state) {
            (SocketKind::Tcp, SocketState::Connected | SocketState::Connecting) => {
                stack.sockets.get_mut::<tcp::Socket>(handle).close();
                meta.state = SocketState::Closed;
                true
            }
            (SocketKind::Tcp, _) | (SocketKind::Udp, _) => {
                stack.metas.remove(&handle);
                stack.udp_loopback.remove(&handle);
                stack.sockets.remove(handle);
                false
            }
        };
        drop(guard);
        if should_poll {
            for _ in 0..4 {
                poll();
                poll_socket_events();
            }
            let mut guard = NETWORK_STACK.lock();
            if let Some(stack) = guard.as_mut() {
                stack.metas.remove(&handle);
                stack.udp_loopback.remove(&handle);
                stack.sockets.remove(handle);
            }
        }
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
                drop(guard);
                for _ in 0..4 {
                    poll();
                    poll_socket_events();
                }
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
        // 只有当底层 smoltcp socket 不再处于 Listen 状态时（即已完成握手），
        // 才表示有真正的入连接。is_active() 对 Listen 状态也返回 true，
        // 所以必须同时检查 !is_listening()。
        with_tcp_socket(handle, |s| s.is_active() && !s.is_listening()).ok_or("stack not initialized")
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
        meta.mcast_groups.clear();
        let recv_timeout_ms = meta.recv_timeout_ms;
        let tcp_nodelay = meta.tcp_nodelay;
        // 创建替换监听 socket
        let rx = tcp::SocketBuffer::new(vec![0; TCP_BUFFER_SIZE]);
        let tx = tcp::SocketBuffer::new(vec![0; TCP_BUFFER_SIZE]);
        let mut new_listener = tcp::Socket::new(rx, tx);
        new_listener.set_nagle_enabled(!tcp_nodelay);
        new_listener.set_ack_delay(if tcp_nodelay {
            None
        } else {
            Some(Duration::from_millis(10))
        });
        let local_ip = meta.local_ip;
        new_listener
            .listen(listen_endpoint(local_ip, port))
            .map_err(|_| "failed to create replacement listener")?;
        let new_h = stack.sockets.add(new_listener);
        stack.metas.insert(new_h, SocketMeta {
            kind: SocketKind::Tcp,
            state: SocketState::Listening { port },
            local_ip,
            local_port: port,
            is_listener: true,
            peer_ip: [0; 4],
            peer_port: 0,
            recv_timeout_ms,
            tcp_nodelay,
            mcast_groups: BTreeSet::new(),
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
        if meta.local_port != 0 {
            Ok(meta.local_port)
        } else {
            match meta.state {
                SocketState::Bound { port } | SocketState::Listening { port } => Ok(port),
                _ => Err("socket not bound"),
            }
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
