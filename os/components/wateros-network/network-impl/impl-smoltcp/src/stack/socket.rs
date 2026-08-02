//! 与传输协议无关的 socket 操作与 TCP/UDP 分派。

use smoltcp::iface::SocketHandle;
use smoltcp::socket::{tcp, udp};
use smoltcp::wire::{IpAddress, IpListenEndpoint};

use super::poll::{poll, poll_socket_events};
use super::state::{NetworkStack, NETWORK_STACK};
use super::tcp::{tcp_is_accept_ready, tcp_is_connected};
use super::types::{
    Ipv4Endpoint, NetworkError, SocketKind, SocketPollSnapshot, SocketSendError, SocketState,
};
use super::udp::{ensure_udp_bound, socket_recvfrom, socket_sendto};

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


pub(super) fn next_ephemeral_port(stack : &mut NetworkStack) -> u16 {
    let port = stack.ephemeral_port;
    stack.ephemeral_port = stack.ephemeral_port
                                .wrapping_add(1);
    if stack.ephemeral_port == 0 {
        stack.ephemeral_port = 49152;
    }
    port
}


/// 将 socket 绑定到本机地址/端口。None 表示 0.0.0.0 wildcard。
/// TCP 仅记录本地端点；真正监听在 [`socket_listen`] 中执行。
pub fn socket_bind(handle : SocketHandle,
                   local_ip : Option<[u8; 4]>,
                   port : u16)
                   -> Result<(), NetworkError> {
    let mut guard = NETWORK_STACK.lock();
    let stack = guard.as_mut()
                     .ok_or(NetworkError::StackUnavailable)?;
    if !is_valid_local_addr(local_ip, stack.local_ip) {
        return Err(NetworkError::AddressNotAvailable);
    }
    // 先只读获取 socket 类型，避免后续与 next_ephemeral_port 的借用冲突
    let kind = stack.metas
                    .get(&handle)
                    .ok_or(NetworkError::InvalidSocket)?
                    .kind;
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
            let meta = stack.metas
                            .get_mut(&handle)
                            .ok_or(NetworkError::InvalidSocket)?;
            meta.state = SocketState::Bound { port : actual_port };
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
                 .map_err(|_| NetworkError::AddressInUse)?;
            let meta = stack.metas
                            .get_mut(&handle)
                            .ok_or(NetworkError::InvalidSocket)?;
            meta.state = SocketState::Bound { port : actual_port };
            meta.local_ip = local_ip;
            meta.local_port = actual_port;
        }
    }
    Ok(())
}


/// 获取 socket 的类型。
pub fn socket_kind(handle : SocketHandle) -> Result<SocketKind, NetworkError> {
    let guard = NETWORK_STACK.lock();
    let stack = guard.as_ref()
                     .ok_or(NetworkError::StackUnavailable)?;
    stack.metas
         .get(&handle)
         .map(|m| m.kind)
         .ok_or(NetworkError::InvalidSocket)
}


/// 获取 socket 的状态。
pub fn socket_state(handle : SocketHandle) -> Result<SocketState, NetworkError> {
    let guard = NETWORK_STACK.lock();
    let stack = guard.as_ref()
                     .ok_or(NetworkError::StackUnavailable)?;
    stack.metas
         .get(&handle)
         .map(|m| m.state)
         .ok_or(NetworkError::InvalidSocket)
}


/// 在同一次协议栈临界区内取得 poll/read/write 所需的完整状态。
pub fn socket_poll_snapshot(handle : SocketHandle) -> Result<SocketPollSnapshot, NetworkError> {
    let mut guard = NETWORK_STACK.lock();
    let stack = guard.as_mut()
                     .ok_or(NetworkError::StackUnavailable)?;
    let (kind, state, is_listener, listener_group) = {
        let meta = stack.metas
                        .get(&handle)
                        .ok_or(NetworkError::InvalidSocket)?;
        (meta.kind, meta.state, meta.is_listener, meta.listener_group)
    };

    match kind {
        SocketKind::Tcp => {
            let has_pending_accept = listener_group.and_then(|group_id| {
                                                       stack.tcp_listener_groups
                                                            .get(&group_id)
                                                   })
                                                   .map(|group| {
                                                       group.handles
                                                            .clone()
                                                   })
                                                   .is_some_and(|handles| {
                                                       handles.into_iter().any(|slot| {
                        tcp_is_accept_ready(stack.sockets.get_mut::<tcp::Socket>(slot))
                    })
                                                   });
            let socket = stack.sockets
                              .get_mut::<tcp::Socket>(handle);
            Ok(SocketPollSnapshot { kind,
                                    state,
                                    can_recv : socket.can_recv(),
                                    may_recv : socket.may_recv(),
                                    may_send : socket.may_send(),
                                    send_capacity : socket.send_capacity(),
                                    is_connected : tcp_is_connected(socket),
                                    has_pending_accept : is_listener && has_pending_accept })
        }
        SocketKind::Udp => {
            let loopback_ready = stack.udp_loopback
                                      .get(&handle)
                                      .is_some_and(|queue| !queue.is_empty());
            let socket = stack.sockets
                              .get_mut::<udp::Socket>(handle);
            let socket_ready = socket.can_recv();
            let may_send = socket.can_send();
            let send_capacity = socket.payload_send_capacity()
                                      .saturating_sub(socket.send_queue());
            Ok(SocketPollSnapshot { kind,
                                    state,
                                    can_recv : loopback_ready || socket_ready,
                                    may_recv : true,
                                    may_send,
                                    send_capacity,
                                    is_connected : matches!(state, SocketState::Connected),
                                    has_pending_accept : false })
        }
    }
}


/// 发起 TCP/UDP connect。TCP 非阻塞返回后需 poll 驱动握手完成；UDP 只记录默认 peer。
pub fn socket_connect(handle : SocketHandle, ip : [u8; 4], port : u16) -> Result<(), NetworkError> {
    use smoltcp::wire::IpAddress;
    let mut guard = NETWORK_STACK.lock();
    let stack = guard.as_mut()
                     .ok_or(NetworkError::StackUnavailable)?;
    let (kind, state, local_ip, bound_port) = {
        let meta = stack.metas
                        .get(&handle)
                        .ok_or(NetworkError::InvalidSocket)?;
        (meta.kind, meta.state, meta.local_ip, meta.local_port)
    };
    match kind {
        SocketKind::Tcp => {
            let local_port = match state {
                SocketState::Created => next_ephemeral_port(stack),
                SocketState::Bound { .. } if bound_port != 0 => bound_port,
                _ => return Err(NetworkError::InvalidState),
            };
            let cx = stack.iface
                          .context();
            let socket = stack.sockets
                              .get_mut::<tcp::Socket>(handle);
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
            if let Some(meta) = stack.metas
                                     .get_mut(&handle)
            {
                meta.state = SocketState::Connecting;
                meta.local_port = local_port;
            }
        }
        SocketKind::Udp => {
            if matches!(state, SocketState::Created) {
                ensure_udp_bound(stack, handle)?;
            }
            if let Some(meta) = stack.metas
                                     .get_mut(&handle)
            {
                meta.state = SocketState::Connected;
            }
        }
    }
    let meta = stack.metas
                    .get_mut(&handle)
                    .ok_or(NetworkError::InvalidSocket)?;
    meta.peer_ip = ip;
    meta.peer_port = port;
    Ok(())
}


/// socket 当前是否可以把数据写入发送缓冲。
pub fn socket_may_send(handle : SocketHandle) -> Result<bool, NetworkError> {
    let mut guard = NETWORK_STACK.lock();
    let stack = guard.as_mut()
                     .ok_or(NetworkError::StackUnavailable)?;
    let kind = stack.metas
                    .get(&handle)
                    .ok_or(NetworkError::InvalidSocket)?
                    .kind;
    Ok(match kind {
        SocketKind::Tcp => stack.sockets
                                .get_mut::<tcp::Socket>(handle)
                                .may_send(),
        SocketKind::Udp => stack.sockets
                                .get_mut::<udp::Socket>(handle)
                                .can_send(),
    })
}


/// socket 当前发送缓冲还能容纳的字节数。
pub fn socket_send_capacity(handle : SocketHandle) -> Result<usize, NetworkError> {
    let mut guard = NETWORK_STACK.lock();
    let stack = guard.as_mut()
                     .ok_or(NetworkError::StackUnavailable)?;
    let kind = stack.metas
                    .get(&handle)
                    .ok_or(NetworkError::InvalidSocket)?
                    .kind;
    Ok(match kind {
        SocketKind::Tcp => stack.sockets
                                .get_mut::<tcp::Socket>(handle)
                                .send_capacity(),
        SocketKind::Udp => {
            let socket = stack.sockets
                              .get_mut::<udp::Socket>(handle);
            socket.payload_send_capacity()
                  .saturating_sub(socket.send_queue())
        }
    })
}


/// 从 socket 发送数据（TCP 和已 connect 的 UDP）。
pub fn socket_send(handle : SocketHandle, data : &[u8]) -> Result<usize, SocketSendError> {
    let mut guard = NETWORK_STACK.lock();
    let stack = guard.as_mut()
                     .ok_or(SocketSendError::StackUnavailable)?;
    let meta = stack.metas
                    .get(&handle)
                    .ok_or(SocketSendError::InvalidSocket)?;
    match meta.kind {
        SocketKind::Tcp => stack.sockets
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


/// 从 socket 接收数据（TCP 和已 connect 的 UDP）。
pub fn socket_recv(handle : SocketHandle, buf : &mut [u8]) -> Result<usize, NetworkError> {
    let mut guard = NETWORK_STACK.lock();
    let stack = guard.as_mut()
                     .ok_or(NetworkError::StackUnavailable)?;
    let meta = stack.metas
                    .get(&handle)
                    .ok_or(NetworkError::InvalidSocket)?;
    match meta.kind {
        SocketKind::Tcp => stack.sockets
                                .get_mut::<tcp::Socket>(handle)
                                .recv_slice(buf)
                                .map_err(|_| NetworkError::Io),
        SocketKind::Udp => {
            drop(guard);
            socket_recvfrom(handle, buf).map(|(n, _, _)| n)
        }
    }
}


/// 关闭 socket。
///
/// UDP 和未建立连接的 TCP 可以立即移除。已建立的 TCP 需要保留在
/// `SocketSet` 中继续完成 FIN/ACK 状态机，待 smoltcp 进入 `Closed`
/// 后再由 [`poll_socket_events`] 回收。
pub fn socket_close(handle : SocketHandle) -> Result<(), NetworkError> {
    let mut guard = NETWORK_STACK.lock();
    let stack = guard.as_mut()
                     .ok_or(NetworkError::StackUnavailable)?;
    let (kind, listener_group) = stack.metas
                                      .get(&handle)
                                      .map(|meta| (meta.kind, meta.listener_group))
                                      .ok_or(NetworkError::InvalidSocket)?;

    if let Some(group_id) = listener_group {
        let group = stack.tcp_listener_groups
                         .remove(&group_id)
                         .ok_or(NetworkError::Internal)?;
        for slot in group.handles {
            stack.metas
                 .remove(&slot);
            stack.udp_loopback
                 .remove(&slot);
            stack.tcp_close_pending
                 .remove(&slot);
            stack.sockets
                 .remove(slot);
        }
        return Ok(());
    }

    let should_poll = match kind {
        SocketKind::Tcp => {
            let socket = stack.sockets
                              .get_mut::<tcp::Socket>(handle);
            socket.close();
            let closed = socket.state() == tcp::State::Closed;

            // fd 已经关闭，上层元数据应立即失效；只有底层 TCP 状态机可能继续存在。
            stack.metas
                 .remove(&handle);
            stack.udp_loopback
                 .remove(&handle);
            if closed {
                stack.sockets
                     .remove(handle);
            } else {
                stack.tcp_close_pending
                     .insert(handle);
            }
            !closed
        }
        SocketKind::Udp => {
            stack.metas
                 .remove(&handle);
            stack.udp_loopback
                 .remove(&handle);
            stack.sockets
                 .remove(handle);
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
pub fn socket_shutdown(handle : SocketHandle) -> Result<(), NetworkError> {
    let mut guard = NETWORK_STACK.lock();
    let stack = guard.as_mut()
                     .ok_or(NetworkError::StackUnavailable)?;
    let meta = stack.metas
                    .get_mut(&handle)
                    .ok_or(NetworkError::InvalidSocket)?;
    match meta.kind {
        SocketKind::Tcp => {
            stack.sockets
                 .get_mut::<tcp::Socket>(handle)
                 .close();
            meta.state = SocketState::Closed;
            drop(guard);
            poll();
            poll_socket_events();
            Ok(())
        }
        SocketKind::Udp => Err(NetworkError::Unsupported),
    }
}


/// 查询 socket 的对端端点（connect 或 accept 后有效）。
pub fn socket_peer_endpoint(handle : SocketHandle) -> Result<Ipv4Endpoint, NetworkError> {
    let guard = NETWORK_STACK.lock();
    let stack = guard.as_ref()
                     .ok_or(NetworkError::StackUnavailable)?;
    let meta = stack.metas
                    .get(&handle)
                    .ok_or(NetworkError::InvalidSocket)?;
    if meta.peer_ip == [0; 4] && meta.peer_port == 0 {
        return Err(NetworkError::NotConnected);
    }
    Ok(Ipv4Endpoint { address : meta.peer_ip, port : meta.peer_port })
}

/// 兼容原有调用路径的对端地址查询。
pub fn socket_peername(handle : SocketHandle) -> Result<([u8; 4], u16), NetworkError> {
    socket_peer_endpoint(handle).map(|endpoint| (endpoint.address, endpoint.port))
}


/// 对端是否位于 IPv4 loopback 网段。
pub fn socket_peer_is_loopback(handle : SocketHandle) -> Result<bool, NetworkError> {
    let guard = NETWORK_STACK.lock();
    let stack = guard.as_ref()
                     .ok_or(NetworkError::StackUnavailable)?;
    let meta = stack.metas
                    .get(&handle)
                    .ok_or(NetworkError::InvalidSocket)?;
    Ok(meta.peer_ip[0] == 127)
}


/// 查询 socket 当前的本地端点。
///
/// 未绑定或绑定到 wildcard 的 socket 返回 `0.0.0.0`；完成 connect 后返回
/// 实际选择的本机地址，loopback 连接返回 `127.0.0.1`。
pub fn socket_local_endpoint(handle : SocketHandle) -> Result<Ipv4Endpoint, NetworkError> {
    let guard = NETWORK_STACK.lock();
    let stack = guard.as_ref()
                     .ok_or(NetworkError::StackUnavailable)?;
    let meta = stack.metas
                    .get(&handle)
                    .ok_or(NetworkError::InvalidSocket)?;
    let address = match meta.local_ip {
        Some(ip) => ip,
        None if matches!(meta.state, SocketState::Connecting | SocketState::Connected) => {
            if meta.peer_ip[0] == 127 { [127, 0, 0, 1] } else { stack.local_ip }
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

/// 兼容原有调用路径的本地端口查询。
pub fn socket_local_port(handle : SocketHandle) -> Result<u16, NetworkError> {
    socket_local_endpoint(handle).map(|endpoint| endpoint.port)
}
