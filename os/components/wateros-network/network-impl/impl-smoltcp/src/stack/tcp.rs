//! TCP socket 创建、监听、收发与 accept 槽池管理。

use alloc::vec;
use alloc::vec::Vec;
use smoltcp::iface::SocketHandle;
use smoltcp::socket::tcp;
use smoltcp::time::Duration;
use smoltcp::wire::IpAddress;

use super::socket::{listen_endpoint, next_ephemeral_port};
use super::state::{
    new_socket_meta, NetworkStack, TcpListenerGroup, NETWORK_STACK, TCP_BUFFER_SIZE,
    TCP_LISTEN_BACKLOG_MAX,
};
use super::types::{NetworkError, SocketKind, SocketState};

pub(super) fn tcp_listener_slot_count(backlog : usize) -> usize {
    // Linux defines backlog as the queue of fully established connections
    // still waiting for accept(). The connection currently being accepted
    // is not part of that queue. Keep one transition slot in addition to
    // the requested queue depth so a replacement listener is available
    // while a userspace server handles the accepted connection.
    backlog.max(1)
           .saturating_add(1)
           .min(TCP_LISTEN_BACKLOG_MAX)
}


/// 对 TCP socket 执行操作。返回 `None` 表示协议栈尚未初始化。
pub fn with_tcp_socket<R>(handle : SocketHandle,
                          f : impl FnOnce(&mut tcp::Socket) -> R)
                          -> Option<R> {
    let mut guard = NETWORK_STACK.lock();
    let stack = guard.as_mut()?;
    Some(f(stack.sockets
                .get_mut::<tcp::Socket>(handle)))
}


/// 创建 TCP socket，返回其 smoltcp 句柄。
pub fn create_tcp_socket() -> Result<SocketHandle, NetworkError> {
    let mut guard = NETWORK_STACK.lock();
    let stack = guard.as_mut()
                     .ok_or(NetworkError::StackUnavailable)?;
    let rx = tcp::SocketBuffer::new(vec![0; TCP_BUFFER_SIZE]);
    let tx = tcp::SocketBuffer::new(vec![0; TCP_BUFFER_SIZE]);
    let socket = tcp::Socket::new(rx, tx);
    let h = stack.sockets
                 .add(socket);
    stack.metas
         .insert(h, new_socket_meta(SocketKind::Tcp));
    Ok(h)
}


/// TCP 三次握手已经完成，连接至少曾进入可传输数据的状态。
pub(super) fn tcp_is_connected(socket : &tcp::Socket) -> bool {
    matches!(socket.state(),
             tcp::State::Established |
             tcp::State::FinWait1 |
             tcp::State::FinWait2 |
             tcp::State::CloseWait)
}


/// 监听 socket 只有完成握手后才可被 accept；`SynReceived` 还不能交给用户态。
pub(super) fn tcp_is_accept_ready(socket : &tcp::Socket) -> bool {
    matches!(socket.state(),
             tcp::State::Established | tcp::State::CloseWait)
}


fn new_tcp_listener_socket(local_ip : Option<[u8; 4]>,
                           port : u16,
                           tcp_nodelay : bool)
                           -> Result<tcp::Socket<'static>, NetworkError> {
    let rx = tcp::SocketBuffer::new(vec![0; TCP_BUFFER_SIZE]);
    let tx = tcp::SocketBuffer::new(vec![0; TCP_BUFFER_SIZE]);
    let mut socket = tcp::Socket::new(rx, tx);
    socket.set_nagle_enabled(!tcp_nodelay);
    socket.set_ack_delay(if tcp_nodelay {
                             None
                         } else {
                             Some(Duration::from_millis(10))
                         });
    socket.listen(listen_endpoint(local_ip, port))
          .map_err(|_| NetworkError::AddressInUse)?;
    Ok(socket)
}


fn register_tcp_listener_slot(stack : &mut NetworkStack,
                              socket : tcp::Socket<'static>,
                              group_id : u64,
                              local_ip : Option<[u8; 4]>,
                              port : u16,
                              recv_timeout_ms : Option<u64>,
                              tcp_nodelay : bool,
                              snd_buf_size : i32,
                              rcv_buf_size : i32)
                              -> SocketHandle {
    let handle = stack.sockets
                      .add(socket);
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
    stack.metas
         .insert(handle, meta);
    handle
}


fn add_tcp_listener_slot(stack : &mut NetworkStack,
                         group_id : u64,
                         local_ip : Option<[u8; 4]>,
                         port : u16,
                         recv_timeout_ms : Option<u64>,
                         tcp_nodelay : bool,
                         snd_buf_size : i32,
                         rcv_buf_size : i32)
                         -> Result<SocketHandle, NetworkError> {
    let socket = new_tcp_listener_socket(local_ip, port, tcp_nodelay)?;
    Ok(register_tcp_listener_slot(stack,
                                  socket,
                                  group_id,
                                  local_ip,
                                  port,
                                  recv_timeout_ms,
                                  tcp_nodelay,
                                  snd_buf_size,
                                  rcv_buf_size))
}


/// TCP socket 开始监听（需先 bind）。
pub fn socket_listen(handle : SocketHandle, backlog : usize) -> Result<(), NetworkError> {
    let mut guard = NETWORK_STACK.lock();
    let stack = guard.as_mut()
                     .ok_or(NetworkError::StackUnavailable)?;
    // 先只读提取端口和本地 IP
    let (mut port, local_ip, recv_timeout_ms, tcp_nodelay, snd_buf_size, rcv_buf_size) = {
        let meta = stack.metas
                        .get(&handle)
                        .ok_or(NetworkError::InvalidSocket)?;
        if meta.kind != SocketKind::Tcp {
            return Err(NetworkError::WrongSocketType);
        }
        let port = match meta.state {
            SocketState::Bound { port } => port,
            _ => return Err(NetworkError::NotBound),
        };
        (port,
         meta.local_ip,
         meta.recv_timeout_ms,
         meta.tcp_nodelay,
         meta.snd_buf_size,
         meta.rcv_buf_size)
    };
    // 若 bind 时指定 port=0，自动分配 ephemeral port
    if port == 0 {
        port = next_ephemeral_port(stack);
        let meta = stack.metas
                        .get_mut(&handle)
                        .ok_or(NetworkError::InvalidSocket)?;
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
         .map_err(|_| NetworkError::AddressInUse)?;
    let meta = stack.metas
                    .get_mut(&handle)
                    .ok_or(NetworkError::InvalidSocket)?;
    meta.state = SocketState::Listening { port };
    meta.local_port = port;
    meta.is_listener = true;
    let group_id = stack.next_listener_group;
    stack.next_listener_group = stack.next_listener_group
                                     .wrapping_add(1)
                                     .max(1);
    meta.listener_group = Some(group_id);

    let mut handles = Vec::with_capacity(slot_count);
    handles.push(handle);
    for socket in prepared_slots {
        let slot = register_tcp_listener_slot(stack,
                                              socket,
                                              group_id,
                                              local_ip,
                                              port,
                                              recv_timeout_ms,
                                              tcp_nodelay,
                                              snd_buf_size,
                                              rcv_buf_size);
        handles.push(slot);
    }
    stack.tcp_listener_groups
         .insert(group_id, TcpListenerGroup { handles });
    Ok(())
}


/// TCP connect 是否已建立。
pub fn socket_is_connected(handle : SocketHandle) -> Result<bool, NetworkError> {
    with_tcp_socket(handle, |socket| {
        tcp_is_connected(socket)
    }).ok_or(NetworkError::StackUnavailable)
}


/// TCP socket 是否可以接收。
pub fn socket_may_recv(handle : SocketHandle) -> Result<bool, NetworkError> {
    with_tcp_socket(handle, |s| s.may_recv()).ok_or(NetworkError::StackUnavailable)
}


/// TCP socket 当前是否有数据可读。
pub fn socket_can_recv(handle : SocketHandle) -> Result<bool, NetworkError> {
    let mut guard = NETWORK_STACK.lock();
    let stack = guard.as_mut()
                     .ok_or(NetworkError::StackUnavailable)?;
    let meta = stack.metas
                    .get(&handle)
                    .ok_or(NetworkError::InvalidSocket)?;
    if meta.recv_reservation
           .is_some()
    {
        return Ok(false);
    }
    Ok(stack.sockets
            .get_mut::<tcp::Socket>(handle)
            .can_recv())
}


/// 从 listener 槽池取出一个已建立连接，并立即补充新的监听槽。
/// 返回 (已建立连接的 socket_handle, 新监听 socket_handle, 对端 IP, 对端端口)。
pub fn socket_accept(handle : SocketHandle)
                     -> Result<(SocketHandle, SocketHandle, [u8; 4], u16), NetworkError> {
    let mut guard = NETWORK_STACK.lock();
    let stack = guard.as_mut()
                     .ok_or(NetworkError::StackUnavailable)?;
    let (group_id, port, local_ip, recv_timeout_ms, tcp_nodelay, snd_buf_size, rcv_buf_size) = {
        let meta = stack.metas
                        .get(&handle)
                        .ok_or(NetworkError::InvalidSocket)?;
        if !meta.is_listener {
            return Err(NetworkError::NotListening);
        }
        let port = match meta.state {
            SocketState::Listening { port } => port,
            _ => return Err(NetworkError::NotListening),
        };
        (meta.listener_group
             .ok_or(NetworkError::Internal)?,
         port,
         meta.local_ip,
         meta.recv_timeout_ms,
         meta.tcp_nodelay,
         meta.snd_buf_size,
         meta.rcv_buf_size)
    };
    let listener_slots = stack.tcp_listener_groups
                              .get(&group_id)
                              .ok_or(NetworkError::Internal)?
                              .handles
                              .clone();
    let established = listener_slots.into_iter()
                                    .find(|&slot| {
                                        tcp_is_accept_ready(stack.sockets
                                                                 .get_mut::<tcp::Socket>(slot))
                                    })
                                    .ok_or(NetworkError::NoPendingConnection)?;
    let (peer_ip, peer_port) = {
        let tcp = stack.sockets
                       .get_mut::<tcp::Socket>(established);
        let remote = tcp.remote_endpoint()
                        .ok_or(NetworkError::Internal)?;
        let peer_ip = match remote.addr {
            IpAddress::Ipv4(ip) => ip.octets(),
        };
        if peer_ip[0] == 127 {
            tcp.set_nagle_enabled(false);
        }
        (peer_ip, remote.port)
    };
    // 取出的监听槽变为普通已连接 socket。
    let meta = stack.metas
                    .get_mut(&established)
                    .unwrap();
    meta.state = SocketState::Connected;
    meta.is_listener = false;
    meta.listener_group = None;
    meta.peer_ip = peer_ip;
    meta.peer_port = peer_port;
    meta.mcast_groups
        .clear();

    {
        let group = stack.tcp_listener_groups
                         .get_mut(&group_id)
                         .ok_or(NetworkError::Internal)?;
        group.handles
             .retain(|&slot| slot != established);
    }
    let new_listener = add_tcp_listener_slot(stack,
                                             group_id,
                                             local_ip,
                                             port,
                                             recv_timeout_ms,
                                             tcp_nodelay,
                                             snd_buf_size,
                                             rcv_buf_size).map_err(|_| NetworkError::Internal)?;
    stack.tcp_listener_groups
         .get_mut(&group_id)
         .ok_or(NetworkError::Internal)?
         .handles
         .push(new_listener);

    // 若 fd 当前指向的正是被 accept 的槽，切换到组内任一新监听槽。
    let replacement = if established == handle {
        stack.tcp_listener_groups
             .get(&group_id)
             .and_then(|group| {
                 group.handles
                      .first()
             })
             .copied()
             .ok_or(NetworkError::Internal)?
    } else {
        handle
    };
    Ok((established, replacement, peer_ip, peer_port))
}

#[cfg(test)]
mod tests {
    use super::{tcp_listener_slot_count, TCP_LISTEN_BACKLOG_MAX};

    #[test]
    fn listener_slot_count_honors_cagent_backlog() {
        assert_eq!(tcp_listener_slot_count(0), 2);
        assert_eq!(tcp_listener_slot_count(1), 2);
        assert_eq!(tcp_listener_slot_count(10), 11);
        assert_eq!(tcp_listener_slot_count(TCP_LISTEN_BACKLOG_MAX + 1),
                   TCP_LISTEN_BACKLOG_MAX);
    }
}
