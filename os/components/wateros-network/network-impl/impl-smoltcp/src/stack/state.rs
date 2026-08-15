//! 协议栈全局状态及各 socket 的内核侧元数据。

use crate::adapter::SmoltcpAdapter;
use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::vec;
use alloc::vec::Vec;
use smoltcp::iface::{Interface, SocketHandle, SocketSet};

use super::types::{NetworkAddress, SocketConnectError, SocketDomain, SocketKind, SocketState};

pub(super) struct SocketMeta {
    pub(super) domain : SocketDomain,
    pub(super) kind : SocketKind,
    pub(super) state : SocketState,
    /// None 表示绑定到当前地址族的 wildcard 地址。
    pub(super) local_ip : Option<NetworkAddress>,
    /// TCP listener 实际交给 smoltcp 匹配的地址；wildcard listener 的槽位
    /// 会分别绑定配置地址和 loopback，同时 `local_ip` 仍保持 None。
    pub(super) listen_ip : Option<NetworkAddress>,
    pub(super) local_port : u16,
    /// TCP 监听槽标记；被 accept 取走后变为普通已连接 socket。
    pub(super) is_listener : bool,
    /// 所属 TCP listener 槽池；一个监听 fd 可对应多个 smoltcp socket。
    pub(super) listener_group : Option<u64>,
    /// 对端地址（connect 发起时或 accept 完成时填入）。
    pub(super) peer_ip : NetworkAddress,
    pub(super) peer_port : u16,
    /// TCP 三次握手是否至少成功过一次；不能仅凭已填写对端地址判断已连接。
    pub(super) connection_established : bool,
    /// 异步 connect 的待取错误；`getsockopt(SO_ERROR)` 读取后清除。
    pub(super) connect_error : Option<SocketConnectError>,
    /// 仅约束 TCP 建连阶段，握手成功后会立即取消协议栈超时。
    pub(super) connect_deadline_ms : Option<i64>,
    /// SO_RCVTIMEO 毫秒值；`None` 表示默认阻塞等待。
    pub(super) recv_timeout_ms : Option<u64>,
    /// TCP_NODELAY 是否启用。
    pub(super) tcp_nodelay : bool,
    /// bind 前设置的地址/端口复用选项；用于补足 smoltcp 未执行的端口冲突检查。
    pub(super) reuse_addr : bool,
    pub(super) reuse_port : bool,
    /// IPv6 packet-info 控制消息类型。0 表示关闭，2 为 RFC 2292 兼容类型，
    /// 50 为当前 Linux IPV6_PKTINFO 类型。
    pub(super) ipv6_pktinfo_type : i32,
    /// IPv4 组播成员（`MCAST_JOIN_GROUP` / `IP_ADD_MEMBERSHIP`）。
    pub(super) mcast_groups : BTreeSet<u32>,
    /// `setsockopt(SO_SNDBUF)` 记录值，供 `getsockopt` / iperf 查询。
    pub(super) snd_buf_size : i32,
    /// `setsockopt(SO_RCVBUF)` 记录值，供 `getsockopt` / iperf 查询。
    pub(super) rcv_buf_size : i32,
    /// 同一时刻仅允许一个 read/recv/recvfrom 持有接收队列前缀。
    pub(super) recv_reservation : Option<u64>,
    pub(super) next_recv_reservation : u64,
    /// smoltcp ICMP socket 按 Echo identifier 分流；第一次发送时从 ICMP
    /// 报文头提取并固定，确保同机多个 ping 进程不会互相收包。
    pub(super) icmp_ident : Option<u16>,
}

impl SocketMeta {
    pub(super) fn new(domain : SocketDomain, kind : SocketKind) -> Self {
        Self { domain,
               kind,
               state : SocketState::Created,
               local_ip : None,
               listen_ip : None,
               local_port : 0,
               is_listener : false,
               listener_group : None,
               peer_ip : NetworkAddress::unspecified(domain),
               peer_port : 0,
               connection_established : false,
               connect_error : None,
               connect_deadline_ms : None,
               recv_timeout_ms : None,
               tcp_nodelay : false,
               reuse_addr : false,
               reuse_port : false,
               ipv6_pktinfo_type : 0,
               mcast_groups : BTreeSet::new(),
               snd_buf_size : default_snd_buf_size(kind),
               rcv_buf_size : default_rcv_buf_size(kind),
               recv_reservation : None,
               next_recv_reservation : 1,
               icmp_ident : None }
    }
}

pub(super) struct TcpListenerGroup {
    pub(super) handles : Vec<SocketHandle>,
}

pub(super) struct LoopbackUdpPacket {
    pub(super) data : Vec<u8>,
    pub(super) source_ip : NetworkAddress,
    pub(super) source_port : u16,
    pub(super) destination_ip : NetworkAddress,
}

/// ICMP socket 没有 peek API。收到的报文先移到此处，用户复制成功后才
/// 删除，因此 MSG_PEEK 和用户指针 fault 都不会误消费数据。
pub(super) struct PendingIcmpPacket {
    pub(super) data : Vec<u8>,
    pub(super) source_ip : NetworkAddress,
}

#[derive(Default)]
pub(super) struct LoopbackUdpQueue {
    packets : VecDeque<LoopbackUdpPacket>,
    queued_bytes : usize,
}

impl LoopbackUdpQueue {
    /// 模拟 UDP socket 的有限接收缓冲。缓冲已满时丢弃新报文，保留已经
    /// 排队的数据报及其 FIFO 顺序。
    pub(super) fn try_push(&mut self,
                           data : &[u8],
                           source_ip : NetworkAddress,
                           source_port : u16,
                           destination_ip : NetworkAddress)
                           -> bool {
        let packet_len = data.len();
        if self.packets.len() >= UDP_LOOPBACK_QUEUE_PACKET_LIMIT ||
           self.queued_bytes
               .checked_add(packet_len)
               .is_none_or(|bytes| bytes > UDP_PACKET_DATA_SIZE)
        {
            return false;
        }
        self.queued_bytes += packet_len;
        self.packets
            .push_back(LoopbackUdpPacket { data : data.to_vec(),
                                           source_ip,
                                           source_port,
                                           destination_ip });
        true
    }

    pub(super) fn pop_front(&mut self) -> Option<LoopbackUdpPacket> {
        let packet = self.packets
                         .pop_front()?;
        self.queued_bytes -= packet.data.len();
        Some(packet)
    }

    pub(super) fn front(&self) -> Option<&LoopbackUdpPacket> { self.packets.front() }

    pub(super) fn is_empty(&self) -> bool {
        self.packets
            .is_empty()
    }
}

/// 协议栈全局状态和动态 socket 管理数据。
pub(super) struct NetworkStack {
    /// smoltcp 与实际网卡之间的设备适配层。
    pub(super) adapter : SmoltcpAdapter,
    /// smoltcp 网络接口，持有 IP 地址、路由表与邻居缓存。
    pub(super) iface : Interface,
    /// smoltcp 的 TCP/UDP/ICMP socket 集合。
    pub(super) sockets : SocketSet<'static>,
    /// WaterOS 为补充 Linux socket 语义维护的元数据。
    pub(super) metas : BTreeMap<SocketHandle, SocketMeta>,
    /// `listen(backlog)` 对应的并发监听槽池。
    pub(super) tcp_listener_groups : BTreeMap<u64, TcpListenerGroup>,
    /// fd 已关闭、但仍需完成 TCP FIN 状态机的底层 socket。
    pub(super) tcp_close_pending : BTreeSet<SocketHandle>,
    /// 已投递到本机 UDP socket、等待用户态接收的有限队列。
    pub(super) udp_loopback : BTreeMap<SocketHandle, LoopbackUdpQueue>,
    pub(super) icmp_pending : BTreeMap<SocketHandle, PendingIcmpPacket>,
    pub(super) local_ipv4 : [u8; 4],
    pub(super) local_ipv6 : Option<[u8; 16]>,
    /// 最近一次交给 smoltcp 的单调毫秒时间，防止无时钟轮询使时间倒退。
    pub(super) last_poll_millis : i64,
    /// 临时端口分配器。
    pub(super) ephemeral_port : u16,
    pub(super) next_listener_group : u64,
}

impl NetworkStack {
    pub(super) fn new(adapter : SmoltcpAdapter,
                      iface : Interface,
                      local_ipv4 : [u8; 4],
                      local_ipv6 : Option<[u8; 16]>)
                      -> Self {
        Self { adapter,
               iface,
               sockets : SocketSet::new(vec![]),
               metas : BTreeMap::new(),
               tcp_listener_groups : BTreeMap::new(),
               tcp_close_pending : BTreeSet::new(),
               udp_loopback : BTreeMap::new(),
               icmp_pending : BTreeMap::new(),
               local_ipv4,
               local_ipv6,
               last_poll_millis : 0,
               ephemeral_port : 49152,
               next_listener_group : 1 }
    }

    pub(super) fn socket_meta(&self,
                              handle : SocketHandle)
                              -> Result<&SocketMeta, super::types::NetworkError> {
        self.metas
            .get(&handle)
            .ok_or(super::types::NetworkError::InvalidSocket)
    }

    pub(super) fn socket_meta_mut(&mut self,
                                  handle : SocketHandle)
                                  -> Result<&mut SocketMeta, super::types::NetworkError> {
        self.metas
            .get_mut(&handle)
            .ok_or(super::types::NetworkError::InvalidSocket)
    }

    pub(super) fn next_ephemeral_port(&mut self) -> u16 {
        let port = self.ephemeral_port;
        self.ephemeral_port = self.ephemeral_port
                                  .wrapping_add(1);
        if self.ephemeral_port == 0 {
            self.ephemeral_port = 49152;
        }
        port
    }

    pub(super) fn configured_address(&self, domain : SocketDomain) -> Option<NetworkAddress> {
        match domain {
            SocketDomain::Ipv4 => Some(NetworkAddress::Ipv4(self.local_ipv4)),
            SocketDomain::Ipv6 => self.local_ipv6
                                      .map(NetworkAddress::Ipv6),
        }
    }
}

/// smoltcp 0.12 的 `last_scaled_window()` 没有处理窗口缩放向下取整后，
/// 新 ACK 极少量越过旧通告窗口右边界的情况，会在序列号减法处 panic。
/// 接收缓冲保持在未缩放窗口可表达的上限；发送缓冲使用相同大小以节省内存。
pub(super) const TCP_RX_BUFFER_SIZE : usize = u16::MAX as usize;
pub(super) const TCP_TX_BUFFER_SIZE : usize = u16::MAX as usize;
pub(super) const UDP_PACKET_DATA_SIZE : usize = 64 * 1024;
pub(super) const UDP_PACKET_METADATA_COUNT : usize = 64;
pub(super) const ICMP_PACKET_DATA_SIZE : usize = 64 * 1024;
pub(super) const ICMP_PACKET_METADATA_COUNT : usize = 16;

/// 临时迁移开关：true 时本机 UDP 也进入 smoltcp，由 SmoltcpAdapter
/// 回灌本地帧；false 时回退到旧的 udp_loopback 数据报队列。
pub(super) const UDP_USE_SMOLTCP_LOOPBACK : bool = false;

/// 防止零长度/极小数据报只消耗队列元数据而绕过字节限额；正常 MTU
/// 数据报仍主要受 64 KiB 总字节数约束。
pub(super) const UDP_LOOPBACK_QUEUE_PACKET_LIMIT : usize = 256;

/// IPv4 最大 UDP payload：65535 - 20 字节 IPv4 头 - 8 字节 UDP 头。
pub(super) const UDP_MAX_PAYLOAD_SIZE : usize = 65_507;
/// IPv6 payload length 包含 UDP 头，因此最大数据为 65535 - 8。
pub(super) const UDP6_MAX_PAYLOAD_SIZE : usize = 65_527;
pub(super) const TCP_MSS : u32 = 1460;

/// 每个监听槽都带约 64 KiB 接收、发送缓冲，限制槽数以约束内核内存。
///
/// CAgent 的本地 HTTP server 使用 backlog 10；上限必须至少覆盖该并发量，
/// 否则首轮连接会在所有监听槽进入 Established 后丢失 SYN。
pub(super) const TCP_LISTEN_BACKLOG_MAX : usize = 16;

fn default_snd_buf_size(kind : SocketKind) -> i32 {
    match kind {
        SocketKind::Tcp => TCP_TX_BUFFER_SIZE as i32,
        SocketKind::Udp => UDP_PACKET_DATA_SIZE as i32,
        SocketKind::Icmp => ICMP_PACKET_DATA_SIZE as i32,
    }
}

fn default_rcv_buf_size(kind : SocketKind) -> i32 {
    match kind {
        SocketKind::Tcp => TCP_RX_BUFFER_SIZE as i32,
        SocketKind::Udp => UDP_PACKET_DATA_SIZE as i32,
        SocketKind::Icmp => ICMP_PACKET_DATA_SIZE as i32,
    }
}
