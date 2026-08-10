//! 与传输协议无关的 socket 操作与 TCP/UDP 分派。

use smoltcp::iface::SocketHandle;
use smoltcp::socket::{tcp, udp};
use smoltcp::wire::{IpAddress, IpListenEndpoint};

use super::global::{with_stack, with_stack_mut};
use super::poll::{poll, poll_socket_events};
use super::state::NetworkStack;
use super::tcp::{tcp_is_accept_ready, tcp_is_connected};
use super::types::{
    Ipv4Endpoint, NetworkError, NetworkSocketSnapshot, SocketKind, SocketPollSnapshot,
    SocketSendError, SocketState,
};

/// 与 syscall 阻塞 connect 的兜底时间一致；成功建连后会取消，不影响长连接空闲时间。
const TCP_CONNECT_TIMEOUT_MS : u64 = 30_000;

fn is_valid_local_addr(addr : Option<[u8; 4]>, configured : [u8; 4]) -> bool {
    match addr {
        None => true,
        Some(ip) => ip == configured || ip[0] == 127,
    }
}


pub(super) fn listen_endpoint(addr : Option<[u8; 4]>, port : u16) -> IpListenEndpoint {
    IpListenEndpoint { addr : addr.map(|ip| IpAddress::v4(ip[0], ip[1], ip[2], ip[3])),
                       port }
}

fn normalize_connect_ip(ip : [u8; 4]) -> [u8; 4] {
    if ip == [0; 4] {
        [127, 0, 0, 1]
    } else {
        ip
    }
}


impl NetworkStack {
    fn socket_table_snapshot(&mut self) -> alloc::vec::Vec<NetworkSocketSnapshot> {
        let handles = self.metas
                          .keys()
                          .copied()
                          .collect::<alloc::vec::Vec<_>>();
        let mut seen_listener_groups = alloc::collections::BTreeSet::new();
        let mut snapshots = alloc::vec::Vec::new();
        for handle in handles {
            let (kind, state, local_ip, local_port, peer_ip, peer_port, listener_group) = {
                let meta = match self.metas.get(&handle) {
                    Some(meta) => meta,
                    None => continue,
                };
                (meta.kind,
                 meta.state,
                 meta.local_ip,
                 meta.local_port,
                 meta.peer_ip,
                 meta.peer_port,
                 meta.listener_group)
            };
            if let Some(group) = listener_group {
                if !seen_listener_groups.insert(group) {
                    continue;
                }
            }
            let address = local_ip.unwrap_or_else(|| {
                if peer_ip[0] == 127 {
                    [127, 0, 0, 1]
                } else if matches!(state, SocketState::Connecting | SocketState::Connected) {
                    self.local_ip
                } else {
                    [0; 4]
                }
            });
            let (tx_queue, rx_queue) = match kind {
                SocketKind::Tcp => {
                    let socket = self.sockets.get::<tcp::Socket>(handle);
                    (socket.send_queue(), socket.recv_queue())
                }
                SocketKind::Udp => {
                    let socket = self.sockets.get::<udp::Socket>(handle);
                    (socket.send_queue(), socket.recv_queue())
                }
            };
            snapshots.push(NetworkSocketSnapshot {
                kind,
                state,
                local : Ipv4Endpoint { address, port : local_port },
                peer : Ipv4Endpoint { address : peer_ip, port : peer_port },
                tx_queue,
                rx_queue,
            });
        }
        snapshots
    }

    // 绑定与基本状态。
    fn bind(&mut self,
            handle : SocketHandle,
            local_ip : Option<[u8; 4]>,
            port : u16)
            -> Result<(), NetworkError> {
        if !is_valid_local_addr(local_ip, self.local_ip) {
            return Err(NetworkError::AddressNotAvailable);
        }
        // 先只读获取 socket 类型，避免后续与 next_ephemeral_port 的借用冲突
        let kind = self.socket_meta(handle)?
                       .kind;
        match kind {
            SocketKind::Tcp => {
                // smoltcp 的 TCP listen 拒绝 port=0，且 getsockname 在 listen 之前
                // 就可能被调用（netperf 服务端流程：bind→getsockname→listen），
                // 因此必须在此处预分配 ephemeral port。
                let actual_port = if port == 0 {
                    self.next_ephemeral_port()
                } else {
                    port
                };
                let meta = self.socket_meta_mut(handle)?;
                meta.state = SocketState::Bound { port : actual_port };
                meta.local_ip = local_ip;
                meta.local_port = actual_port;
            }
            SocketKind::Udp => {
                // smoltcp 的 UDP bind 拒绝 port=0，必须预分配 ephemeral port
                let actual_port = if port == 0 {
                    self.next_ephemeral_port()
                } else {
                    port
                };
                self.sockets
                    .get_mut::<udp::Socket>(handle)
                    .bind(listen_endpoint(local_ip, actual_port))
                    .map_err(|_| NetworkError::AddressInUse)?;
                let meta = self.socket_meta_mut(handle)?;
                meta.state = SocketState::Bound { port : actual_port };
                meta.local_ip = local_ip;
                meta.local_port = actual_port;
            }
        }
        Ok(())
    }

    fn kind(&self, handle : SocketHandle) -> Result<SocketKind, NetworkError> {
        self.socket_meta(handle)
            .map(|meta| meta.kind)
    }

    // 单次加锁取得读、写和连接状态，供 poll/read/write 共用。
    fn poll_snapshot(&mut self, handle : SocketHandle) -> Result<SocketPollSnapshot, NetworkError> {
        let (kind, state, is_listener, listener_group, recv_reserved, connect_error) = {
            let meta = self.socket_meta(handle)?;
            (meta.kind,
             meta.state,
             meta.is_listener,
             meta.listener_group,
             meta.recv_reservation
                 .is_some(),
             meta.connect_error)
        };

        match kind {
            SocketKind::Tcp => {
                let has_pending_accept = listener_group.and_then(|group_id| {
                                                           self.tcp_listener_groups
                                                               .get(&group_id)
                                                       })
                                                       .map(|group| {
                                                           group.handles
                                                                .clone()
                                                       })
                                                       .is_some_and(|handles| {
                                                           handles.into_iter().any(|slot| {
                        tcp_is_accept_ready(self.sockets.get_mut::<tcp::Socket>(slot))
                    })
                                                       });
                let socket = self.sockets
                                 .get_mut::<tcp::Socket>(handle);
                let may_send = socket.may_send();
                let send_capacity = socket.send_capacity()
                                          .saturating_sub(socket.send_queue());
                Ok(SocketPollSnapshot { kind,
                                        state,
                                        can_recv : !recv_reserved && socket.can_recv(),
                                        may_recv : socket.may_recv(),
                                        may_send,
                                        send_capacity,
                                        is_connected : tcp_is_connected(socket),
                                        connect_error,
                                        has_pending_accept : is_listener && has_pending_accept })
            }
            SocketKind::Udp => {
                let loopback_ready = self.udp_loopback
                                         .get(&handle)
                                         .is_some_and(|queue| !queue.is_empty());
                let socket = self.sockets
                                 .get_mut::<udp::Socket>(handle);
                let socket_ready = socket.can_recv();
                let may_send = socket.can_send();
                let send_capacity = socket.payload_send_capacity()
                                          .saturating_sub(socket.send_queue());
                Ok(SocketPollSnapshot { kind,
                                        state,
                                        can_recv : !recv_reserved &&
                                                   (loopback_ready || socket_ready),
                                        may_recv : true,
                                        may_send,
                                        send_capacity,
                                        is_connected : matches!(state, SocketState::Connected),
                                        connect_error : None,
                                        has_pending_accept : false })
            }
        }
    }

    // 连接与发送。
    fn connect(&mut self,
               handle : SocketHandle,
               ip : [u8; 4],
               port : u16)
               -> Result<(), NetworkError> {
        let (kind, state, local_ip, bound_port) = {
            let meta = self.socket_meta(handle)?;
            (meta.kind, meta.state, meta.local_ip, meta.local_port)
        };
        match kind {
            SocketKind::Tcp => {
                let local_port = match state {
                    SocketState::Created => self.next_ephemeral_port(),
                    SocketState::Bound { .. } if bound_port != 0 => bound_port,
                    _ => return Err(NetworkError::InvalidState),
                };
                let connect_deadline_ms = self.last_poll_millis
                                              .saturating_add(TCP_CONNECT_TIMEOUT_MS as i64);
                let cx = self.iface.context();
                let socket = self.sockets
                                 .get_mut::<tcp::Socket>(handle);
                socket.set_timeout(Some(smoltcp::time::Duration::from_millis(
                    TCP_CONNECT_TIMEOUT_MS,
                )));
                if ip[0] == 127 {
                    // 回环仍经过 Ethernet MTU 分段；禁用 Nagle 可让同一次
                    // send() 的尾部短段无需等待首段 ACK。
                    socket.set_nagle_enabled(false);
                }
                socket.connect(cx,
                               (IpAddress::v4(ip[0], ip[1], ip[2], ip[3]), port),
                               listen_endpoint(local_ip, local_port))
                      .map_err(|e| {
                          log::warn!("[network-stack] connect err: {:?}, local_port={}",
                                     e,
                                     local_port);
                          NetworkError::ConnectionRefused
                      })?;
                if let Some(meta) = self.metas
                                        .get_mut(&handle)
                {
                    meta.state = SocketState::Connecting;
                    meta.local_port = local_port;
                    meta.connection_established = false;
                    meta.connect_error = None;
                    meta.connect_deadline_ms = Some(connect_deadline_ms);
                }
            }
            SocketKind::Udp => {
                if matches!(state, SocketState::Created) {
                    self.ensure_udp_bound(handle)?;
                }
                if let Some(meta) = self.metas
                                        .get_mut(&handle)
                {
                    meta.state = SocketState::Connected;
                    meta.connection_established = true;
                    meta.connect_error = None;
                    meta.connect_deadline_ms = None;
                }
            }
        }
        let meta = self.socket_meta_mut(handle)?;
        meta.peer_ip = ip;
        meta.peer_port = port;
        Ok(())
    }

    pub(super) fn send_capacity(&mut self, handle : SocketHandle) -> Result<usize, NetworkError> {
        let kind = self.socket_meta(handle)?
                       .kind;
        Ok(match kind {
            SocketKind::Tcp => self.sockets
                                   .get_mut::<tcp::Socket>(handle)
                                   .send_capacity(),
            SocketKind::Udp => {
                let socket = self.sockets
                                 .get_mut::<udp::Socket>(handle);
                socket.payload_send_capacity()
                      .saturating_sub(socket.send_queue())
            }
        })
    }

    fn send(&mut self, handle : SocketHandle, data : &[u8]) -> Result<usize, SocketSendError> {
        let (kind, peer_ip, peer_port) = self.metas
                                             .get(&handle)
                                             .map(|meta| (meta.kind, meta.peer_ip, meta.peer_port))
                                             .ok_or(SocketSendError::InvalidSocket)?;
        match kind {
            SocketKind::Tcp => self.sockets
                                   .get_mut::<tcp::Socket>(handle)
                                   .send_slice(data)
                                   .map_err(|_| SocketSendError::NotConnected),
            SocketKind::Udp => {
                if peer_ip == [0; 4] && peer_port == 0 {
                    return Err(SocketSendError::NotConnected);
                }
                self.send_udp_to(handle, data, peer_ip, peer_port)
            }
        }
    }

    // 生命周期。
    fn close(&mut self, handle : SocketHandle) -> Result<bool, NetworkError> {
        let (kind, listener_group) = self.metas
                                         .get(&handle)
                                         .map(|meta| (meta.kind, meta.listener_group))
                                         .ok_or(NetworkError::InvalidSocket)?;

        if let Some(group_id) = listener_group {
            let group = self.tcp_listener_groups
                            .remove(&group_id)
                            .ok_or(NetworkError::Internal)?;
            for slot in group.handles {
                self.metas
                    .remove(&slot);
                self.udp_loopback
                    .remove(&slot);
                self.tcp_close_pending
                    .remove(&slot);
                self.sockets
                    .remove(slot);
            }
            return Ok(false);
        }

        let should_poll = match kind {
            SocketKind::Tcp => {
                let socket = self.sockets
                                 .get_mut::<tcp::Socket>(handle);
                socket.close();
                let closed = socket.state() == tcp::State::Closed;

                // fd 已经关闭，上层元数据应立即失效；只有底层 TCP 状态机可能继续存在。
                self.metas
                    .remove(&handle);
                self.udp_loopback
                    .remove(&handle);
                if closed {
                    self.sockets
                        .remove(handle);
                } else {
                    self.tcp_close_pending
                        .insert(handle);
                }
                !closed
            }
            SocketKind::Udp => {
                self.metas
                    .remove(&handle);
                self.udp_loopback
                    .remove(&handle);
                self.sockets
                    .remove(handle);
                false
            }
        };
        Ok(should_poll)
    }

    fn shutdown(&mut self, handle : SocketHandle) -> Result<bool, NetworkError> {
        let kind = self.socket_meta(handle)?
                       .kind;
        match kind {
            SocketKind::Tcp => {
                self.sockets
                    .get_mut::<tcp::Socket>(handle)
                    .close();
                self.socket_meta_mut(handle)?
                    .state = SocketState::Closed;
                Ok(true)
            }
            SocketKind::Udp => Err(NetworkError::Unsupported),
        }
    }

    // 本地与对端地址查询。
    fn peer_endpoint(&self, handle : SocketHandle) -> Result<Ipv4Endpoint, NetworkError> {
        let meta = self.socket_meta(handle)?;
        if !meta.connection_established || (meta.peer_ip == [0; 4] && meta.peer_port == 0) {
            return Err(NetworkError::NotConnected);
        }
        Ok(Ipv4Endpoint { address : meta.peer_ip,
                          port : meta.peer_port })
    }

    fn peer_is_loopback(&self, handle : SocketHandle) -> Result<bool, NetworkError> {
        self.socket_meta(handle)
            .map(|meta| meta.peer_ip[0] == 127)
    }

    fn local_endpoint(&self, handle : SocketHandle) -> Result<Ipv4Endpoint, NetworkError> {
        let meta = self.socket_meta(handle)?;
        let address = match meta.local_ip {
            Some(ip) => ip,
            None if matches!(meta.state,
                             SocketState::Connecting | SocketState::Connected) =>
            {
                if meta.peer_ip[0] == 127 {
                    [127, 0, 0, 1]
                } else {
                    self.local_ip
                }
            }
            None => [0, 0, 0, 0],
        };
        let port = if meta.local_port != 0 {
            meta.local_port
        } else {
            match meta.state {
                SocketState::Bound { port } | SocketState::Listening { port } => port,
                _ => 0,
            }
        };
        Ok(Ipv4Endpoint { address, port })
    }
}

// 对外包装函数集中在文件末尾：它们只负责取得全局协议栈并调用上面的内部方法。

/// 获取 socket 的类型。
pub fn socket_kind(handle : SocketHandle) -> Result<SocketKind, NetworkError> {
    with_stack(NetworkError::StackUnavailable,
               |stack| stack.kind(handle))
}

/// 枚举当前 TCP/UDP socket，供 `/proc/net` 等只读管理接口使用。
pub fn network_socket_table_snapshot()
                                     -> Result<alloc::vec::Vec<NetworkSocketSnapshot>, NetworkError> {
    with_stack_mut(NetworkError::StackUnavailable,
                   |stack| Ok(stack.socket_table_snapshot()))
}

/// 将 socket 绑定到本机地址/端口。None 表示 0.0.0.0 wildcard。
/// TCP 仅记录本地端点；真正监听在 [`socket_listen`] 中执行。
pub fn socket_bind(handle : SocketHandle,
                   local_ip : Option<[u8; 4]>,
                   port : u16)
                   -> Result<(), NetworkError> {
    with_stack_mut(NetworkError::StackUnavailable,
                   |stack| stack.bind(handle, local_ip, port))
}

/// 发起 TCP/UDP connect。TCP 非阻塞返回后需 poll 驱动握手完成；UDP 只记录默认 peer。
pub fn socket_connect(handle : SocketHandle, ip : [u8; 4], port : u16) -> Result<(), NetworkError> {
    // Linux treats INADDR_ANY as the local host when it is used as a
    // connect destination. smoltcp rejects the unspecified address.
    let ip = normalize_connect_ip(ip);
    with_stack_mut(NetworkError::StackUnavailable,
                   |stack| stack.connect(handle, ip, port))
}

/// 在同一次协议栈临界区内取得 poll/read/write 所需的完整状态。
pub fn socket_poll_snapshot(handle : SocketHandle) -> Result<SocketPollSnapshot, NetworkError> {
    with_stack_mut(NetworkError::StackUnavailable,
                   |stack| stack.poll_snapshot(handle))
}

/// 从 socket 发送数据（TCP 和已 connect 的 UDP）。
pub fn socket_send(handle : SocketHandle, data : &[u8]) -> Result<usize, SocketSendError> {
    with_stack_mut(SocketSendError::StackUnavailable,
                   |stack| stack.send(handle, data))
}

/// 关闭 socket。
///
/// UDP 和未建立连接的 TCP 可以立即移除。已建立的 TCP 需要保留在
/// `SocketSet` 中继续完成 FIN/ACK 状态机，待 smoltcp 进入 `Closed`
/// 后再由 [`poll_socket_events`] 回收。
pub fn socket_close(handle : SocketHandle) -> Result<(), NetworkError> {
    let should_poll = with_stack_mut(NetworkError::StackUnavailable,
                                     |stack| stack.close(handle))?;
    if should_poll {
        poll();
        poll_socket_events();
    }
    Ok(())
}

/// 关闭 socket 的通信方向；当前 TCP 以全关闭近似实现，fd 仍由调用方保留。
pub fn socket_shutdown(handle : SocketHandle) -> Result<(), NetworkError> {
    let should_poll = with_stack_mut(NetworkError::StackUnavailable,
                                     |stack| stack.shutdown(handle))?;
    if should_poll {
        poll();
        poll_socket_events();
    }
    Ok(())
}

/// 查询 socket 的对端端点（connect 或 accept 后有效）。
pub fn socket_peer_endpoint(handle : SocketHandle) -> Result<Ipv4Endpoint, NetworkError> {
    with_stack(NetworkError::StackUnavailable,
               |stack| stack.peer_endpoint(handle))
}

/// 对端是否位于 IPv4 loopback 网段。
pub fn socket_peer_is_loopback(handle : SocketHandle) -> Result<bool, NetworkError> {
    with_stack(NetworkError::StackUnavailable,
               |stack| stack.peer_is_loopback(handle))
}

/// 查询 socket 当前的本地端点。
///
/// 未绑定或绑定到 wildcard 的 socket 返回 `0.0.0.0`；完成 connect 后返回
/// 实际选择的本机地址，loopback 连接返回 `127.0.0.1`。
pub fn socket_local_endpoint(handle : SocketHandle) -> Result<Ipv4Endpoint, NetworkError> {
    with_stack(NetworkError::StackUnavailable,
               |stack| stack.local_endpoint(handle))
}

#[cfg(test)]
mod tests {
    use super::normalize_connect_ip;

    #[test]
    fn connect_maps_unspecified_destination_to_loopback() {
        assert_eq!(normalize_connect_ip([0, 0, 0, 0]),
                   [127, 0, 0, 1]);
        assert_eq!(normalize_connect_ip([10, 0, 2, 2]),
                   [10, 0, 2, 2]);
    }
}
