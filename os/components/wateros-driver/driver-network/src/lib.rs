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
pub use socket_handles::{
    SocketReceiveLease, SocketRef, TcpListenerHandle, TcpStreamHandle, UdpSocketHandle,
};

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

    /// socket 发送失败原因；syscall 层据此返回稳定的 Linux errno。
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum SocketSendError {
        MessageTooLarge,
        WouldBlock,
        NoBufferSpace,
        NotConnected,
        InvalidDestination,
        InvalidSocket,
        StackUnavailable,
        Io,
    }

    /// Receive reservation setup/commit failures exposed without smoltcp types.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum SocketRecvError {
        Busy,
        Empty,
        Finished,
        InvalidSocket,
        NoMemory,
        Io,
    }

    /// Result of committing or cancelling a receive lease.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum SocketRecvFinish {
        Bytes(usize),
        Fault,
    }

    pub(crate) struct SocketRecvReservation {
        handle: SocketHandle,
        id: u64,
        kind: SocketKind,
        staged_len: usize,
        datagram_len: usize,
        source_ip: [u8; 4],
        source_port: u16,
        loopback_udp: bool,
    }

    impl SocketRecvReservation {
        pub(crate) fn staged_len(&self) -> usize { self.staged_len }

        pub(crate) fn source(&self) -> ([u8; 4], u16) {
            (self.source_ip, self.source_port)
        }

        pub(crate) fn kind(&self) -> SocketKind { self.kind }

        pub(crate) fn datagram_len(&self) -> usize { self.datagram_len }
    }

    /// 一次协议栈加锁内取得的 socket 就绪状态，避免多核下分次查询观察到不同瞬间。
    #[derive(Clone, Copy, Debug)]
    pub struct SocketPollSnapshot {
        pub kind: SocketKind,
        pub state: SocketState,
        pub can_recv: bool,
        pub may_recv: bool,
        pub may_send: bool,
        pub send_capacity: usize,
        pub is_connected: bool,
        pub has_pending_accept: bool,
    }

    struct SocketMeta {
        kind: SocketKind,
        state: SocketState,
        /// None 表示绑定到 0.0.0.0，即任意本机地址。
        local_ip: Option<[u8; 4]>,
        local_port: u16,
        /// TCP 监听槽标记；被 accept 取走后变为普通已连接 socket。
        is_listener: bool,
        /// 所属 TCP listener 槽池；一个监听 fd 可对应多个 smoltcp socket。
        listener_group: Option<u64>,
        /// 对端地址（connect 发起时或 accept 完成时填入）。
        peer_ip: [u8; 4],
        peer_port: u16,
        /// SO_RCVTIMEO 毫秒值；`None` 表示默认阻塞等待。
        recv_timeout_ms: Option<u64>,
        /// TCP_NODELAY 是否启用。
        tcp_nodelay: bool,
        /// IPv4 组播成员（`MCAST_JOIN_GROUP` / `IP_ADD_MEMBERSHIP`）。
        mcast_groups: BTreeSet<u32>,
        /// `setsockopt(SO_SNDBUF)` 记录值，供 `getsockopt` / iperf 查询。
        snd_buf_size: i32,
        /// `setsockopt(SO_RCVBUF)` 记录值，供 `getsockopt` / iperf 查询。
        rcv_buf_size: i32,
        /// Only one read/recv/recvfrom may own the receive queue prefix.
        recv_reservation: Option<u64>,
        next_recv_reservation: u64,
    }

    struct TcpListenerGroup {
        handles: Vec<SocketHandle>,
    }

    struct LoopbackUdpPacket {
        data: Vec<u8>,
        source_ip: [u8; 4],
        source_port: u16,
    }

    #[derive(Default)]
    struct LoopbackUdpQueue {
        packets: VecDeque<LoopbackUdpPacket>,
        queued_bytes: usize,
    }

    impl LoopbackUdpQueue {
        /// 模拟 UDP socket 的有限接收缓冲。缓冲已满时丢弃新报文，保留已经
        /// 排队的数据报及其 FIFO 顺序。
        fn try_push(&mut self, data: &[u8], source_ip: [u8; 4], source_port: u16) -> bool {
            let packet_len = data.len();
            if self.packets.len() >= UDP_LOOPBACK_QUEUE_PACKET_LIMIT
                || self
                    .queued_bytes
                    .checked_add(packet_len)
                    .is_none_or(|bytes| bytes > UDP_PACKET_DATA_SIZE)
            {
                return false;
            }
            self.queued_bytes += packet_len;
            self.packets.push_back(LoopbackUdpPacket {
                data: data.to_vec(),
                source_ip,
                source_port,
            });
            true
        }

        fn pop_front(&mut self) -> Option<LoopbackUdpPacket> {
            let packet = self.packets.pop_front()?;
            self.queued_bytes -= packet.data.len();
            Some(packet)
        }

        fn front(&self) -> Option<&LoopbackUdpPacket> {
            self.packets.front()
        }

        fn is_empty(&self) -> bool {
            self.packets.is_empty()
        }
    }

    /// 协议栈全局状态 + 动态 socket 管理。
    pub struct NetworkStack {
        adapter: SmoltcpAdapter,
        iface: Interface,
        sockets: SocketSet<'static>,
        metas: BTreeMap<SocketHandle, SocketMeta>,
        /// `listen(backlog)` 对应的并发监听槽池。
        tcp_listener_groups: BTreeMap<u64, TcpListenerGroup>,
        /// fd 已关闭、但仍需完成 TCP FIN 状态机的底层 socket。
        tcp_close_pending: BTreeSet<SocketHandle>,
        /// 已投递到本机 UDP socket、等待用户态接收的有限队列。
        udp_loopback: BTreeMap<SocketHandle, LoopbackUdpQueue>,
        local_ip: [u8; 4],
        ephemeral_port: u16,
        next_listener_group: u64,
    }

    fn debug_cpu_id() -> usize { arch::cpu::current_cpu_id().raw() }

    /// 全局协议栈锁是 socket 卡死时最关键的 wait-for 节点。包装类型保留原有
    /// `.lock()` API，只有 `gdb-debug` 构建会发布 owner/contention。
    static NETWORK_STACK: debug::TrackedMutex<Option<NetworkStack>> =
        debug::TrackedMutex::new(None, debug::DebugLockKind::Network, debug_cpu_id);
    const TCP_BUFFER_SIZE: usize = 256 * 1024;
    const UDP_PACKET_DATA_SIZE: usize = 64 * 1024;
    const UDP_PACKET_METADATA_COUNT: usize = 64;
    /// 临时迁移开关：true 时本机 UDP 也进入 smoltcp，由 SmoltcpAdapter
    /// 回灌本地帧；false 时回退到旧的 udp_loopback 数据报队列。
    const UDP_USE_SMOLTCP_LOOPBACK: bool = false;
    /// 防止零长度/极小数据报只消耗队列元数据而绕过字节限额；正常 MTU
    /// 数据报仍主要受 64 KiB 总字节数约束。
    const UDP_LOOPBACK_QUEUE_PACKET_LIMIT: usize = 256;
    /// IPv4 最大 UDP payload：65535 - 20 字节 IPv4 头 - 8 字节 UDP 头。
    const UDP_MAX_PAYLOAD_SIZE: usize = 65_507;
    const TCP_MSS: u32 = 1460;
    /// 每个监听槽都带 256 KiB 收、发缓冲，限制槽数以约束内核内存。
    ///
    /// CAgent 的本地 HTTP server 使用 backlog 10；上限必须至少覆盖该并发量，
    /// 否则首轮连接会在所有监听槽进入 Established 后丢失 SYN。
    const TCP_LISTEN_BACKLOG_MAX: usize = 16;

    fn tcp_listener_slot_count(backlog: usize) -> usize {
        // Linux defines backlog as the queue of fully established connections
        // still waiting for accept(). The connection currently being accepted
        // is not part of that queue. Keep one transition slot in addition to
        // the requested queue depth so a replacement listener is available
        // while a userspace server handles the accepted connection.
        backlog
            .max(1)
            .saturating_add(1)
            .min(TCP_LISTEN_BACKLOG_MAX)
    }

    fn default_snd_buf_size(kind: SocketKind) -> i32 {
        match kind {
            SocketKind::Tcp => TCP_BUFFER_SIZE as i32,
            SocketKind::Udp => UDP_PACKET_DATA_SIZE as i32,
        }
    }

    fn default_rcv_buf_size(kind: SocketKind) -> i32 {
        default_snd_buf_size(kind)
    }

    fn new_socket_meta(kind: SocketKind) -> SocketMeta {
        SocketMeta {
            kind,
            state: SocketState::Created,
            local_ip: None,
            local_port: 0,
            is_listener: false,
            listener_group: None,
            peer_ip: [0; 4],
            peer_port: 0,
            recv_timeout_ms: None,
            tcp_nodelay: false,
            mcast_groups: BTreeSet::new(),
            snd_buf_size: default_snd_buf_size(kind),
            rcv_buf_size: default_rcv_buf_size(kind),
            recv_reservation: None,
            next_recv_reservation: 1,
        }
    }

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

        let mut stack_slot = NETWORK_STACK.lock();
        if stack_slot.is_some() {
            return Err("network stack already initialized");
        }
        *stack_slot = Some(NetworkStack {
            adapter,
            iface,
            sockets: SocketSet::new(vec![]),
            metas: BTreeMap::new(),
            tcp_listener_groups: BTreeMap::new(),
            tcp_close_pending: BTreeSet::new(),
            udp_loopback: BTreeMap::new(),
            local_ip: ip,
            ephemeral_port: 49152,
            next_listener_group: 1,
        });
        drop(stack_slot);

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
        stack.metas.insert(h, new_socket_meta(SocketKind::Tcp));
        Ok(h)
    }

    /// 创建 UDP socket，返回其 smoltcp 句柄。
    pub fn create_udp_socket() -> Result<SocketHandle, &'static str> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        let rx = udp::PacketBuffer::new(
            vec![udp::PacketMetadata::EMPTY; UDP_PACKET_METADATA_COUNT],
            vec![0; UDP_PACKET_DATA_SIZE],
        );
        let tx = udp::PacketBuffer::new(
            vec![udp::PacketMetadata::EMPTY; UDP_PACKET_METADATA_COUNT],
            vec![0; UDP_PACKET_DATA_SIZE],
        );
        let socket = udp::Socket::new(rx, tx);
        let h = stack.sockets.add(socket);
        stack.metas.insert(h, new_socket_meta(SocketKind::Udp));
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

    /// TCP 三次握手已经完成，连接至少曾进入可传输数据的状态。
    fn tcp_is_connected(socket: &tcp::Socket) -> bool {
        matches!(
            socket.state(),
            tcp::State::Established
                | tcp::State::FinWait1
                | tcp::State::FinWait2
                | tcp::State::CloseWait
        )
    }

    /// 监听 socket 只有完成握手后才可被 accept；`SynReceived` 还不能交给用户态。
    fn tcp_is_accept_ready(socket: &tcp::Socket) -> bool {
        matches!(socket.state(), tcp::State::Established | tcp::State::CloseWait)
    }

    fn new_tcp_listener_socket(
        local_ip: Option<[u8; 4]>,
        port: u16,
        tcp_nodelay: bool,
    ) -> Result<tcp::Socket<'static>, &'static str> {
        let rx = tcp::SocketBuffer::new(vec![0; TCP_BUFFER_SIZE]);
        let tx = tcp::SocketBuffer::new(vec![0; TCP_BUFFER_SIZE]);
        let mut socket = tcp::Socket::new(rx, tx);
        socket.set_nagle_enabled(!tcp_nodelay);
        socket.set_ack_delay(if tcp_nodelay {
            None
        } else {
            Some(Duration::from_millis(10))
        });
        socket
            .listen(listen_endpoint(local_ip, port))
            .map_err(|_| "tcp listen failed")?;
        Ok(socket)
    }

    fn register_tcp_listener_slot(
        stack: &mut NetworkStack,
        socket: tcp::Socket<'static>,
        group_id: u64,
        local_ip: Option<[u8; 4]>,
        port: u16,
        recv_timeout_ms: Option<u64>,
        tcp_nodelay: bool,
        snd_buf_size: i32,
        rcv_buf_size: i32,
    ) -> SocketHandle {
        let handle = stack.sockets.add(socket);
        let mut meta = new_socket_meta(SocketKind::Tcp);
        meta.state = SocketState::Listening { port };
        meta.local_ip = local_ip;
        meta.local_port = port;
        meta.is_listener = true;
        meta.listener_group = Some(group_id);
        meta.recv_timeout_ms = recv_timeout_ms;
        meta.tcp_nodelay = tcp_nodelay;
        meta.snd_buf_size = snd_buf_size;
        meta.rcv_buf_size = rcv_buf_size;
        stack.metas.insert(handle, meta);
        handle
    }

    fn add_tcp_listener_slot(
        stack: &mut NetworkStack,
        group_id: u64,
        local_ip: Option<[u8; 4]>,
        port: u16,
        recv_timeout_ms: Option<u64>,
        tcp_nodelay: bool,
        snd_buf_size: i32,
        rcv_buf_size: i32,
    ) -> Result<SocketHandle, &'static str> {
        let socket = new_tcp_listener_socket(local_ip, port, tcp_nodelay)?;
        Ok(register_tcp_listener_slot(
            stack,
            socket,
            group_id,
            local_ip,
            port,
            recv_timeout_ms,
            tcp_nodelay,
            snd_buf_size,
            rcv_buf_size,
        ))
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

    fn deliver_loopback_udp(
        stack: &mut NetworkStack,
        source_port: u16,
        dest_ip: [u8; 4],
        dest_port: u16,
        data: &[u8],
    ) {
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
            // UDP 不为未来可能 bind 的 socket 暂存数据报。当前没有匹配的接收者
            // 时直接丢弃；发送端仍视为成功，符合无连接 UDP 的发送语义。
            return;
        };
        let queue = stack
            .udp_loopback
            .entry(target)
            .or_insert_with(LoopbackUdpQueue::default);
        let _delivered = queue.try_push(data, source_ip, source_port);
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
    pub fn socket_listen(handle: SocketHandle, backlog: usize) -> Result<(), &'static str> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        // 先只读提取端口和本地 IP
        let (
            mut port,
            local_ip,
            recv_timeout_ms,
            tcp_nodelay,
            snd_buf_size,
            rcv_buf_size,
        ) = {
            let meta = stack.metas.get(&handle).ok_or("invalid socket handle")?;
            if meta.kind != SocketKind::Tcp {
                return Err("not a tcp socket");
            }
            let port = match meta.state {
                SocketState::Bound { port } => port,
                _ => return Err("socket not bound"),
            };
            (
                port,
                meta.local_ip,
                meta.recv_timeout_ms,
                meta.tcp_nodelay,
                meta.snd_buf_size,
                meta.rcv_buf_size,
            )
        };
        // 若 bind 时指定 port=0，自动分配 ephemeral port
        if port == 0 {
            port = next_ephemeral_port(stack);
            let meta = stack.metas.get_mut(&handle).ok_or("invalid socket handle")?;
            meta.state = SocketState::Bound { port };
            meta.local_port = port;
        }
        let slot_count = tcp_listener_slot_count(backlog);
        let mut prepared_slots = Vec::with_capacity(slot_count.saturating_sub(1));
        for _ in 1..slot_count {
            prepared_slots.push(new_tcp_listener_socket(local_ip, port, tcp_nodelay)?);
        }

        // Extra slots are prepared before mutating the caller's socket. A
        // recoverable listen error therefore cannot leave a partial group.
        stack.sockets
            .get_mut::<tcp::Socket>(handle)
            .listen(listen_endpoint(local_ip, port))
            .map_err(|_| "tcp listen failed")?;
        let meta = stack.metas.get_mut(&handle).ok_or("invalid socket handle")?;
        meta.state = SocketState::Listening { port };
        meta.local_port = port;
        meta.is_listener = true;
        let group_id = stack.next_listener_group;
        stack.next_listener_group = stack.next_listener_group.wrapping_add(1).max(1);
        meta.listener_group = Some(group_id);

        let mut handles = Vec::with_capacity(slot_count);
        handles.push(handle);
        for socket in prepared_slots {
            let slot = register_tcp_listener_slot(
                stack,
                socket,
                group_id,
                local_ip,
                port,
                recv_timeout_ms,
                tcp_nodelay,
                snd_buf_size,
                rcv_buf_size,
            );
            handles.push(slot);
        }
        stack
            .tcp_listener_groups
            .insert(group_id, TcpListenerGroup { handles });
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

    /// 在同一次协议栈临界区内取得 poll/read/write 所需的完整状态。
    pub fn socket_poll_snapshot(handle: SocketHandle) -> Result<SocketPollSnapshot, &'static str> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        let (kind, state, is_listener, listener_group, recv_reserved) = {
            let meta = stack.metas.get(&handle).ok_or("invalid socket handle")?;
            (
                meta.kind,
                meta.state,
                meta.is_listener,
                meta.listener_group,
                meta.recv_reservation.is_some(),
            )
        };

        match kind {
            SocketKind::Tcp => {
                let has_pending_accept = listener_group
                    .and_then(|group_id| stack.tcp_listener_groups.get(&group_id))
                    .map(|group| group.handles.clone())
                    .is_some_and(|handles| {
                        handles.into_iter().any(|slot| {
                            tcp_is_accept_ready(stack.sockets.get_mut::<tcp::Socket>(slot))
                        })
                    });
                let socket = stack.sockets.get_mut::<tcp::Socket>(handle);
                Ok(SocketPollSnapshot {
                    kind,
                    state,
                    can_recv: !recv_reserved && socket.can_recv(),
                    may_recv: socket.may_recv(),
                    may_send: socket.may_send(),
                    send_capacity: socket.send_capacity(),
                    is_connected: tcp_is_connected(socket),
                    has_pending_accept: is_listener && has_pending_accept,
                })
            }
            SocketKind::Udp => {
                let loopback_ready = stack
                    .udp_loopback
                    .get(&handle)
                    .is_some_and(|queue| !queue.is_empty());
                let socket = stack.sockets.get_mut::<udp::Socket>(handle);
                let socket_ready = socket.can_recv();
                let may_send = socket.can_send();
                let send_capacity = socket
                    .payload_send_capacity()
                    .saturating_sub(socket.send_queue());
                Ok(SocketPollSnapshot {
                    kind,
                    state,
                    can_recv: !recv_reserved && (loopback_ready || socket_ready),
                    may_recv: true,
                    may_send,
                    send_capacity,
                    is_connected: matches!(state, SocketState::Connected),
                    has_pending_accept: false,
                })
            }
        }
    }

    /// 发起 TCP/UDP connect。TCP 非阻塞返回后需 poll 驱动握手完成；UDP 只记录默认 peer。
    pub fn socket_connect(handle: SocketHandle, ip: [u8; 4], port: u16) -> Result<(), &'static str> {
        use smoltcp::wire::IpAddress;
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        let (kind, state, local_ip, bound_port) = {
            let meta = stack.metas.get(&handle).ok_or("invalid socket handle")?;
            (meta.kind, meta.state, meta.local_ip, meta.local_port)
        };
        match kind {
            SocketKind::Tcp => {
                let local_port = match state {
                    SocketState::Created => next_ephemeral_port(stack),
                    SocketState::Bound { .. } if bound_port != 0 => bound_port,
                    _ => return Err("invalid tcp socket state for connect"),
                };
                let cx = stack.iface.context();
                let socket = stack.sockets.get_mut::<tcp::Socket>(handle);
                if ip[0] == 127 {
                    // 回环仍经过 Ethernet MTU 分段；禁用 Nagle 可让同一次
                    // send() 的尾部短段无需等待首段 ACK。
                    socket.set_nagle_enabled(false);
                }
                socket
                    .connect(
                        cx,
                        (IpAddress::v4(ip[0], ip[1], ip[2], ip[3]), port),
                        listen_endpoint(local_ip, local_port),
                    )
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

    /// TCP connect 是否已建立。
    pub fn socket_is_connected(handle: SocketHandle) -> Result<bool, &'static str> {
        with_tcp_socket(handle, |socket| tcp_is_connected(socket)).ok_or("stack not initialized")
    }

    /// socket 当前是否可以把数据写入发送缓冲。
    pub fn socket_may_send(handle: SocketHandle) -> Result<bool, &'static str> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        let kind = stack.metas.get(&handle).ok_or("invalid socket handle")?.kind;
        Ok(match kind {
            SocketKind::Tcp => stack.sockets.get_mut::<tcp::Socket>(handle).may_send(),
            SocketKind::Udp => stack.sockets.get_mut::<udp::Socket>(handle).can_send(),
        })
    }

    /// socket 当前发送缓冲还能容纳的字节数。
    pub fn socket_send_capacity(handle: SocketHandle) -> Result<usize, &'static str> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        let kind = stack.metas.get(&handle).ok_or("invalid socket handle")?.kind;
        Ok(match kind {
            SocketKind::Tcp => stack.sockets.get_mut::<tcp::Socket>(handle).send_capacity(),
            SocketKind::Udp => {
                let socket = stack.sockets.get_mut::<udp::Socket>(handle);
                socket
                    .payload_send_capacity()
                    .saturating_sub(socket.send_queue())
            }
        })
    }

    /// TCP socket 是否可以接收。
    pub fn socket_may_recv(handle: SocketHandle) -> Result<bool, &'static str> {
        with_tcp_socket(handle, |s| s.may_recv()).ok_or("stack not initialized")
    }

    /// TCP socket 当前是否有数据可读。
    pub fn socket_can_recv(handle: SocketHandle) -> Result<bool, &'static str> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        let meta = stack.metas.get(&handle).ok_or("invalid socket handle")?;
        if meta.recv_reservation.is_some() {
            return Ok(false);
        }
        Ok(stack.sockets.get_mut::<tcp::Socket>(handle).can_recv())
    }

    /// 从 socket 发送数据（TCP 和已 connect 的 UDP）。
    pub fn socket_send(handle: SocketHandle, data: &[u8]) -> Result<usize, SocketSendError> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or(SocketSendError::StackUnavailable)?;
        let meta = stack.metas.get(&handle).ok_or(SocketSendError::InvalidSocket)?;
        match meta.kind {
            SocketKind::Tcp => stack
                .sockets
                .get_mut::<tcp::Socket>(handle)
                .send_slice(data)
                .map_err(|_| SocketSendError::NotConnected),
            SocketKind::Udp => {
                let ip = meta.peer_ip;
                let port = meta.peer_port;
                if ip == [0; 4] && port == 0 {
                    return Err(SocketSendError::NotConnected);
                }
                drop(guard);
                socket_sendto(handle, data, ip, port)
            }
        }
    }

    /// Reserve the receive queue prefix without consuming it.
    pub(crate) fn socket_prepare_recv(
        handle: SocketHandle,
        buf: &mut [u8],
    ) -> Result<SocketRecvReservation, SocketRecvError> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or(SocketRecvError::Io)?;
        let (kind, id, peer_ip, peer_port) = {
            let meta = stack.metas.get_mut(&handle).ok_or(SocketRecvError::InvalidSocket)?;
            if meta.recv_reservation.is_some() {
                return Err(SocketRecvError::Busy);
            }
            let id = meta.next_recv_reservation;
            meta.next_recv_reservation = meta.next_recv_reservation.wrapping_add(1);
            meta.recv_reservation = Some(id);
            (meta.kind, id, meta.peer_ip, meta.peer_port)
        };

        let prepared = match kind {
            SocketKind::Tcp => {
                let socket = stack.sockets.get_mut::<tcp::Socket>(handle);
                let n = match socket.peek_slice(buf) {
                    Ok(n) => n,
                    Err(_) => {
                        if let Some(meta) = stack.metas.get_mut(&handle) {
                            meta.recv_reservation = None;
                        }
                        return Err(SocketRecvError::Io);
                    }
                };
                if n == 0 {
                    if let Some(meta) = stack.metas.get_mut(&handle) {
                        meta.recv_reservation = None;
                    }
                    return if socket.may_recv() {
                        Err(SocketRecvError::Empty)
                    } else {
                        Err(SocketRecvError::Finished)
                    };
                }
                SocketRecvReservation {
                    handle,
                    id,
                    kind,
                    staged_len: n,
                    datagram_len: n,
                    source_ip: peer_ip,
                    source_port: peer_port,
                    loopback_udp: false,
                }
            }
            SocketKind::Udp => {
                if let Some(packet) = stack.udp_loopback.get(&handle)
                                                    .and_then(LoopbackUdpQueue::front) {
                    let n = packet.data.len().min(buf.len());
                    buf[..n].copy_from_slice(&packet.data[..n]);
                    SocketRecvReservation {
                        handle,
                        id,
                        kind,
                        staged_len: n,
                        datagram_len: packet.data.len(),
                        source_ip: packet.source_ip,
                        source_port: packet.source_port,
                        loopback_udp: true,
                    }
                } else {
                    let socket = stack.sockets.get_mut::<udp::Socket>(handle);
                    let (payload, metadata) = match socket.peek() {
                        Ok(value) => value,
                        Err(udp::RecvError::Exhausted) => {
                            if let Some(meta) = stack.metas.get_mut(&handle) {
                                meta.recv_reservation = None;
                            }
                            return Err(SocketRecvError::Empty);
                        }
                        Err(_) => {
                            if let Some(meta) = stack.metas.get_mut(&handle) {
                                meta.recv_reservation = None;
                            }
                            return Err(SocketRecvError::Io);
                        }
                    };
                    let n = payload.len().min(buf.len());
                    buf[..n].copy_from_slice(&payload[..n]);
                    let source_ip = match metadata.endpoint.addr {
                        IpAddress::Ipv4(addr) => addr.octets(),
                    };
                    SocketRecvReservation {
                        handle,
                        id,
                        kind,
                        staged_len: n,
                        datagram_len: payload.len(),
                        source_ip,
                        source_port: metadata.endpoint.port,
                        loopback_udp: false,
                    }
                }
            }
        };
        Ok(prepared)
    }

    /// Commit a copied prefix, or cancel without consuming on an immediate fault.
    pub(crate) fn socket_finish_recv(
        reservation: SocketRecvReservation,
        copied: usize,
        complete: bool,
    ) -> Result<SocketRecvFinish, SocketRecvError> {
        if copied > reservation.staged_len {
            let mut guard = NETWORK_STACK.lock();
            if let Some(stack) = guard.as_mut() {
                if let Some(meta) = stack.metas.get_mut(&reservation.handle) {
                    if meta.recv_reservation == Some(reservation.id) {
                        meta.recv_reservation = None;
                    }
                }
            }
            return Err(SocketRecvError::Io);
        }
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or(SocketRecvError::Io)?;
        let active_matches = stack.metas
                                  .get(&reservation.handle)
                                  .is_some_and(|meta| meta.recv_reservation == Some(reservation.id));
        if !active_matches {
            return Err(SocketRecvError::InvalidSocket);
        }

        if copied == 0 && !complete {
            if let Some(meta) = stack.metas.get_mut(&reservation.handle) {
                meta.recv_reservation = None;
            }
            return Ok(SocketRecvFinish::Fault);
        }

        let consume_result = match reservation.kind {
            SocketKind::Tcp => {
                if copied == 0 {
                    Ok(())
                } else {
                    let socket = stack.sockets.get_mut::<tcp::Socket>(reservation.handle);
                    let mut remaining = copied;
                    let mut result = Ok(());
                    while remaining > 0 {
                        let consumed = match socket.recv(|data| {
                            let n = remaining.min(data.len());
                            (n, n)
                        }) {
                            Ok(consumed) => consumed,
                            Err(_) => {
                                result = Err(SocketRecvError::Io);
                                break;
                            }
                        };
                        if consumed == 0 {
                            result = Err(SocketRecvError::Io);
                            break;
                        }
                        remaining -= consumed;
                    }
                    result
                }
            }
            SocketKind::Udp if reservation.loopback_udp => {
                stack.udp_loopback
                     .get_mut(&reservation.handle)
                     .and_then(LoopbackUdpQueue::pop_front)
                     .map(|_| ())
                     .ok_or(SocketRecvError::Io)
            }
            SocketKind::Udp => stack.sockets
                                    .get_mut::<udp::Socket>(reservation.handle)
                                    .recv()
                                    .map(|_| ())
                                    .map_err(|_| SocketRecvError::Io),
        };
        if let Some(meta) = stack.metas.get_mut(&reservation.handle) {
            meta.recv_reservation = None;
        }
        consume_result?;
        Ok(SocketRecvFinish::Bytes(copied))
    }

    /// From socket receive compatibility path. New syscall paths use receive leases.
    pub fn socket_recv(handle: SocketHandle, buf: &mut [u8]) -> Result<usize, &'static str> {
        let reservation = socket_prepare_recv(handle, buf).map_err(map_recv_error)?;
        let copied = reservation.staged_len();
        match socket_finish_recv(reservation, copied, true).map_err(map_recv_error)? {
            SocketRecvFinish::Bytes(n) => Ok(n),
            SocketRecvFinish::Fault => Err("recv failed"),
        }
    }

    /// UDP sendto。
    pub fn socket_sendto(
        handle: SocketHandle,
        data: &[u8],
        ip: [u8; 4],
        port: u16,
    ) -> Result<usize, SocketSendError> {
        use smoltcp::wire::IpAddress;
        if data.len() > UDP_MAX_PAYLOAD_SIZE {
            return Err(SocketSendError::MessageTooLarge);
        }
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or(SocketSendError::StackUnavailable)?;
        let source_port = ensure_udp_bound(stack, handle).map_err(|err| match err {
            "invalid socket handle" | "not a udp socket" => SocketSendError::InvalidSocket,
            "udp socket not bound" => SocketSendError::NotConnected,
            _ => SocketSendError::Io,
        })?;
        if !UDP_USE_SMOLTCP_LOOPBACK && is_local_destination(ip, stack.local_ip) {
            deliver_loopback_udp(stack, source_port, ip, port, data);
            return Ok(data.len());
        }
        stack
            .sockets
            .get_mut::<udp::Socket>(handle)
            .send_slice(data, (IpAddress::v4(ip[0], ip[1], ip[2], ip[3]), port))
            .map(|()| data.len())
            .map_err(|err| match err {
                udp::SendError::BufferFull => SocketSendError::WouldBlock,
                udp::SendError::Unaddressable => SocketSendError::InvalidDestination,
            })
    }

    /// UDP recvfrom。返回 (字节数, 来源IP, 来源端口)。
    pub fn socket_recvfrom(handle: SocketHandle, buf: &mut [u8]) -> Result<(usize, [u8; 4], u16), &'static str> {
        let reservation = socket_prepare_recv(handle, buf).map_err(map_recv_error)?;
        if reservation.kind() != SocketKind::Udp {
            let _ = socket_finish_recv(reservation, 0, false);
            return Err("not a udp socket");
        }
        let copied = reservation.staged_len();
        let (ip, port) = reservation.source();
        match socket_finish_recv(reservation, copied, true).map_err(map_recv_error)? {
            SocketRecvFinish::Bytes(n) => Ok((n, ip, port)),
            SocketRecvFinish::Fault => Err("recvfrom failed"),
        }
    }

    fn map_recv_error(error: SocketRecvError) -> &'static str {
        match error {
            SocketRecvError::Busy => "recv busy",
            SocketRecvError::Empty => "recv empty",
            SocketRecvError::Finished => "recv finished",
            SocketRecvError::InvalidSocket => "invalid socket handle",
            SocketRecvError::NoMemory => "recv no memory",
            SocketRecvError::Io => "recv failed",
        }
    }

    /// UDP socket 是否有数据可读。
    pub fn socket_udp_can_recv(handle: SocketHandle) -> Result<bool, &'static str> {
        {
            let guard = NETWORK_STACK.lock();
            let stack = guard.as_ref().ok_or("stack not initialized")?;
            let meta = stack.metas.get(&handle).ok_or("invalid socket handle")?;
            if meta.recv_reservation.is_some() {
                return Ok(false);
            }
            if stack
                .udp_loopback
                .get(&handle)
                .is_some_and(|queue| !queue.is_empty())
            {
                return Ok(true);
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
        if optval.len() >= 16 {
            let family = u16::from_ne_bytes([optval[8], optval[9]]);
            if family == 2 {
                return Ok(u32::from_ne_bytes([
                    optval[12], optval[13], optval[14], optval[15],
                ]));
            }
        }
        if optval.len() >= 12 {
            let family = u16::from_ne_bytes([optval[4], optval[5]]);
            if family == 2 {
                return Ok(u32::from_ne_bytes([
                    optval[8], optval[9], optval[10], optval[11],
                ]));
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
        const SO_DONTROUTE: usize = 5;
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
        // 对回环目标没有网关可绕行，SO_DONTROUTE 的开启与关闭不会改变
        // 当前数据路径；仍解析布尔参数，避免把畸形 optval 当作成功。
        if level == SOL_SOCKET && optname == SO_DONTROUTE {
            let _enabled = sockopt_bool(optval)?;
            return Ok(());
        }
        // netperf/iperf 会 setsockopt(SO_SNDBUF/SO_RCVBUF)；记录请求值供 getsockopt 回报。
        if level == SOL_SOCKET && optname == SO_SNDBUF {
            let value = sockopt_i32(optval)?;
            let mut guard = NETWORK_STACK.lock();
            let stack = guard.as_mut().ok_or("stack not initialized")?;
            let meta = stack.metas.get_mut(&handle).ok_or("invalid socket handle")?;
            meta.snd_buf_size = value.max(0);
            return Ok(());
        }
        if level == SOL_SOCKET && optname == SO_RCVBUF {
            let value = sockopt_i32(optval)?;
            let mut guard = NETWORK_STACK.lock();
            let stack = guard.as_mut().ok_or("stack not initialized")?;
            let meta = stack.metas.get_mut(&handle).ok_or("invalid socket handle")?;
            meta.rcv_buf_size = value.max(0);
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

        let rcv_space = {
            let guard = NETWORK_STACK.lock();
            guard
                .as_ref()
                .and_then(|stack| stack.metas.get(&handle))
                .map(|meta| meta.rcv_buf_size.max(0) as u32)
                .unwrap_or(TCP_BUFFER_SIZE as u32)
        };

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
        write_u32(&mut out, 96, rcv_space); // tcpi_rcv_space
        write_u32(&mut out, 100, 0); // tcpi_total_retrans
        write_u32(&mut out, 228, rcv_space); // tcpi_snd_wnd on newer Linux
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
        if level == SOL_SOCKET && optname == SO_SNDBUF {
            let value = {
                let guard = NETWORK_STACK.lock();
                let stack = guard.as_ref().ok_or("stack not initialized")?;
                let meta = stack.metas.get(&handle).ok_or("invalid socket handle")?;
                meta.snd_buf_size
            };
            return Ok(value.to_ne_bytes().to_vec());
        }
        if level == SOL_SOCKET && optname == SO_RCVBUF {
            let value = {
                let guard = NETWORK_STACK.lock();
                let stack = guard.as_ref().ok_or("stack not initialized")?;
                let meta = stack.metas.get(&handle).ok_or("invalid socket handle")?;
                meta.rcv_buf_size
            };
            return Ok(value.to_ne_bytes().to_vec());
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
        Err("unsupported sockopt")
    }

    /// 关闭 socket。
    ///
    /// UDP 和未建立连接的 TCP 可以立即移除。已建立的 TCP 需要保留在
    /// `SocketSet` 中继续完成 FIN/ACK 状态机，待 smoltcp 进入 `Closed`
    /// 后再由 [`poll_socket_events`] 回收。
    pub fn socket_close(handle: SocketHandle) -> Result<(), &'static str> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        let (kind, listener_group) = stack
            .metas
            .get(&handle)
            .map(|meta| (meta.kind, meta.listener_group))
            .ok_or("invalid socket handle")?;

        if let Some(group_id) = listener_group {
            let group = stack
                .tcp_listener_groups
                .remove(&group_id)
                .ok_or("invalid listener group")?;
            for slot in group.handles {
                stack.metas.remove(&slot);
                stack.udp_loopback.remove(&slot);
                stack.tcp_close_pending.remove(&slot);
                stack.sockets.remove(slot);
            }
            return Ok(());
        }

        let should_poll = match kind {
            SocketKind::Tcp => {
                let socket = stack.sockets.get_mut::<tcp::Socket>(handle);
                socket.close();
                let closed = socket.state() == tcp::State::Closed;

                // fd 已经关闭，上层元数据应立即失效；只有底层 TCP 状态机可能继续存在。
                stack.metas.remove(&handle);
                stack.udp_loopback.remove(&handle);
                if closed {
                    stack.sockets.remove(handle);
                } else {
                    stack.tcp_close_pending.insert(handle);
                }
                !closed
            }
            SocketKind::Udp => {
                stack.metas.remove(&handle);
                stack.udp_loopback.remove(&handle);
                stack.sockets.remove(handle);
                false
            }
        };
        drop(guard);
        if should_poll {
            poll();
            poll_socket_events();
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
                poll();
                poll_socket_events();
                Ok(())
            }
            SocketKind::Udp => Err("shutdown unsupported for udp"),
        }
    }

    /// 从 listener 槽池取出一个已建立连接，并立即补充新的监听槽。
    /// 返回 (已建立连接的 socket_handle, 新监听 socket_handle, 对端 IP, 对端端口)。
    pub fn socket_accept(
        handle: SocketHandle,
    ) -> Result<(SocketHandle, SocketHandle, [u8; 4], u16), &'static str> {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut().ok_or("stack not initialized")?;
        let (
            group_id,
            port,
            local_ip,
            recv_timeout_ms,
            tcp_nodelay,
            snd_buf_size,
            rcv_buf_size,
        ) = {
            let meta = stack.metas.get(&handle).ok_or("invalid socket handle")?;
            if !meta.is_listener {
                return Err("not a listening socket");
            }
            let port = match meta.state {
                SocketState::Listening { port } => port,
                _ => return Err("not listening"),
            };
            (
                meta.listener_group.ok_or("listener group missing")?,
                port,
                meta.local_ip,
                meta.recv_timeout_ms,
                meta.tcp_nodelay,
                meta.snd_buf_size,
                meta.rcv_buf_size,
            )
        };
        let listener_slots = stack
            .tcp_listener_groups
            .get(&group_id)
            .ok_or("invalid listener group")?
            .handles
            .clone();
        let established = listener_slots
            .into_iter()
            .find(|&slot| tcp_is_accept_ready(stack.sockets.get_mut::<tcp::Socket>(slot)))
            .ok_or("no pending connection")?;
        let (peer_ip, peer_port) = {
            let tcp = stack.sockets.get_mut::<tcp::Socket>(established);
            let remote = tcp.remote_endpoint().ok_or("accepted socket has no peer")?;
            let peer_ip = match remote.addr {
                IpAddress::Ipv4(ip) => ip.octets(),
            };
            if peer_ip[0] == 127 {
                tcp.set_nagle_enabled(false);
            }
            (peer_ip, remote.port)
        };
        // 取出的监听槽变为普通已连接 socket。
        let meta = stack.metas.get_mut(&established).unwrap();
        meta.state = SocketState::Connected;
        meta.is_listener = false;
        meta.listener_group = None;
        meta.peer_ip = peer_ip;
        meta.peer_port = peer_port;
        meta.mcast_groups.clear();

        {
            let group = stack
                .tcp_listener_groups
                .get_mut(&group_id)
                .ok_or("invalid listener group")?;
            group.handles.retain(|&slot| slot != established);
        }
        let new_listener = add_tcp_listener_slot(
            stack,
            group_id,
            local_ip,
            port,
            recv_timeout_ms,
            tcp_nodelay,
            snd_buf_size,
            rcv_buf_size,
        )
        .map_err(|_| "failed to create replacement listener")?;
        stack
            .tcp_listener_groups
            .get_mut(&group_id)
            .ok_or("invalid listener group")?
            .handles
            .push(new_listener);

        // 若 fd 当前指向的正是被 accept 的槽，切换到组内任一新监听槽。
        let replacement = if established == handle {
            stack
                .tcp_listener_groups
                .get(&group_id)
                .and_then(|group| group.handles.first())
                .copied()
                .ok_or("listener group empty")?
        } else {
            handle
        };
        Ok((established, replacement, peer_ip, peer_port))
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

    /// 对端是否位于 IPv4 loopback 网段。
    pub fn socket_peer_is_loopback(handle: SocketHandle) -> Result<bool, &'static str> {
        let guard = NETWORK_STACK.lock();
        let stack = guard.as_ref().ok_or("stack not initialized")?;
        let meta = stack.metas.get(&handle).ok_or("invalid socket handle")?;
        Ok(meta.peer_ip[0] == 127)
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

    /// poll 后调用：更新 socket 状态，并回收已完成 TCP 关闭状态机的底层 socket。
    pub fn poll_socket_events() {
        let mut guard = NETWORK_STACK.lock();
        let stack = match guard.as_mut() {
            Some(s) => s,
            None => return,
        };
        // 检查 Connecting → Connected/Closed 转换。RST 或重传耗尽后必须把
        // 失败状态同步到元数据，阻塞 connect 才能退出而不是永久等待。
        let mut updated: BTreeMap<SocketHandle, SocketState> = BTreeMap::new();
        for (&h, meta) in &stack.metas {
            if meta.state == SocketState::Connecting {
                let socket = stack.sockets.get_mut::<tcp::Socket>(h);
                if tcp_is_connected(socket) {
                    updated.insert(h, SocketState::Connected);
                } else if socket.state() == tcp::State::Closed {
                    updated.insert(h, SocketState::Closed);
                }
            }
        }
        for (h, s) in updated {
            if let Some(meta) = stack.metas.get_mut(&h) {
                meta.state = s;
            }
        }

        let closed: Vec<SocketHandle> = stack
            .tcp_close_pending
            .iter()
            .copied()
            .filter(|&h| stack.sockets.get_mut::<tcp::Socket>(h).state() == tcp::State::Closed)
            .collect();
        for h in closed {
            stack.tcp_close_pending.remove(&h);
            stack.sockets.remove(h);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{tcp_listener_slot_count, TCP_LISTEN_BACKLOG_MAX};

        #[test]
        fn listener_slot_count_honors_cagent_backlog() {
            assert_eq!(tcp_listener_slot_count(0), 2);
            assert_eq!(tcp_listener_slot_count(1), 2);
            assert_eq!(tcp_listener_slot_count(10), 11);
            assert_eq!(
                tcp_listener_slot_count(TCP_LISTEN_BACKLOG_MAX + 1),
                TCP_LISTEN_BACKLOG_MAX
            );
        }
    }
}
