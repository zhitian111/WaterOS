//! UDP socket 创建、数据报收发与本机回环队列。

use alloc::vec;
use smoltcp::iface::SocketHandle;
use smoltcp::socket::udp;

use super::socket::{listen_endpoint, next_ephemeral_port};
use super::state::{
    new_socket_meta, LoopbackUdpQueue, NetworkStack, NETWORK_STACK, UDP_MAX_PAYLOAD_SIZE,
    UDP_PACKET_DATA_SIZE, UDP_PACKET_METADATA_COUNT, UDP_USE_SMOLTCP_LOOPBACK,
};
use super::types::{NetworkError, SocketKind, SocketSendError, SocketState};

/// 对 UDP socket 执行操作。返回 `None` 表示协议栈尚未初始化。
pub fn with_udp_socket<R>(handle : SocketHandle,
                          f : impl FnOnce(&mut udp::Socket) -> R)
                          -> Option<R> {
    let mut guard = NETWORK_STACK.lock();
    let stack = guard.as_mut()?;
    Some(f(stack.sockets
                .get_mut::<udp::Socket>(handle)))
}


/// 创建 UDP socket，返回其 smoltcp 句柄。
pub fn create_udp_socket() -> Result<SocketHandle, NetworkError> {
    let mut guard = NETWORK_STACK.lock();
    let stack = guard.as_mut()
                     .ok_or(NetworkError::StackUnavailable)?;
    let rx = udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; UDP_PACKET_METADATA_COUNT],
                                    vec![0; UDP_PACKET_DATA_SIZE]);
    let tx = udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; UDP_PACKET_METADATA_COUNT],
                                    vec![0; UDP_PACKET_DATA_SIZE]);
    let socket = udp::Socket::new(rx, tx);
    let h = stack.sockets
                 .add(socket);
    stack.metas
         .insert(h, new_socket_meta(SocketKind::Udp));
    Ok(h)
}


fn is_local_destination(ip : [u8; 4], configured : [u8; 4]) -> bool {
    ip[0] == 127 || ip == configured
}


fn local_addr_matches(bound : Option<[u8; 4]>, dest : [u8; 4], configured : [u8; 4]) -> bool {
    match bound {
        None => true,
        Some(ip) if ip[0] == 127 && dest[0] == 127 => true,
        Some(ip) => ip == dest || (dest[0] == 127 && ip == configured),
    }
}


pub(super) fn ensure_udp_bound(stack : &mut NetworkStack,
                               handle : SocketHandle)
                               -> Result<u16, NetworkError> {
    let (kind, state, local_ip) = {
        let meta = stack.metas
                        .get(&handle)
                        .ok_or(NetworkError::InvalidSocket)?;
        (meta.kind, meta.state, meta.local_ip)
    };
    if kind != SocketKind::Udp {
        return Err(NetworkError::WrongSocketType);
    }
    match state {
        SocketState::Bound { port } => Ok(port),
        SocketState::Connected => {
            let port = stack.metas
                            .get(&handle)
                            .ok_or(NetworkError::InvalidSocket)?
                            .local_port;
            if port == 0 {
                Err(NetworkError::NotBound)
            } else {
                Ok(port)
            }
        }
        SocketState::Created => {
            let local_port = next_ephemeral_port(stack);
            stack.sockets
                 .get_mut::<udp::Socket>(handle)
                 .bind(listen_endpoint(local_ip, local_port))
                 .map_err(|_| NetworkError::AddressInUse)?;
            if let Some(meta) = stack.metas
                                     .get_mut(&handle)
            {
                meta.state = SocketState::Bound { port : local_port };
                meta.local_port = local_port;
            }
            Ok(local_port)
        }
        _ => Err(NetworkError::NotBound),
    }
}


fn deliver_loopback_udp(stack : &mut NetworkStack,
                        source_port : u16,
                        dest_ip : [u8; 4],
                        dest_port : u16,
                        data : &[u8]) {
    let source_ip = if dest_ip[0] == 127 {
        [127, 0, 0, 1]
    } else {
        stack.local_ip
    };
    let connected_target =
        stack.metas
             .iter()
             .find_map(|(&h, meta)| {
                 if meta.kind != SocketKind::Udp {
                     return None;
                 }
                 match meta.state {
                     SocketState::Connected
                         if meta.local_port == dest_port &&
                            local_addr_matches(meta.local_ip, dest_ip, stack.local_ip) &&
                            meta.peer_port == source_port &&
                            (meta.peer_ip == source_ip ||
                             (meta.peer_ip[0] == 127 && source_ip[0] == 127)) =>
                     {
                         Some(h)
                     }
                     _ => None,
                 }
             });
    let target = connected_target.or_else(|| {
                                     stack.metas
                                          .iter()
                                          .find_map(|(&h, meta)| {
                                              if meta.kind != SocketKind::Udp {
                                                  return None;
                                              }
                                              match meta.state {
                                                  SocketState::Bound { port }
                                                      if port == dest_port &&
                                                         local_addr_matches(meta.local_ip,
                                                                            dest_ip,
                                                                            stack.local_ip) =>
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
    let queue = stack.udp_loopback
                     .entry(target)
                     .or_insert_with(LoopbackUdpQueue::default);
    let _delivered = queue.try_push(data, source_ip, source_port);
}


/// UDP sendto。
pub fn socket_sendto(handle : SocketHandle,
                     data : &[u8],
                     ip : [u8; 4],
                     port : u16)
                     -> Result<usize, SocketSendError> {
    use smoltcp::wire::IpAddress;
    if data.len() > UDP_MAX_PAYLOAD_SIZE {
        return Err(SocketSendError::MessageTooLarge);
    }
    let mut guard = NETWORK_STACK.lock();
    let stack = guard.as_mut()
                     .ok_or(SocketSendError::StackUnavailable)?;
    let source_port =
        ensure_udp_bound(stack, handle).map_err(|err| match err {
                                           NetworkError::InvalidSocket | NetworkError::WrongSocketType => {
                                               SocketSendError::InvalidSocket
                                           }
                                           NetworkError::NotBound => SocketSendError::NotConnected,
                                           _ => SocketSendError::Io,
                                       })?;
    if !UDP_USE_SMOLTCP_LOOPBACK && is_local_destination(ip, stack.local_ip) {
        deliver_loopback_udp(stack, source_port, ip, port, data);
        return Ok(data.len());
    }
    stack.sockets
         .get_mut::<udp::Socket>(handle)
         .send_slice(data,
                     (IpAddress::v4(ip[0], ip[1], ip[2], ip[3]), port))
         .map(|()| data.len())
         .map_err(|err| match err {
             udp::SendError::BufferFull => SocketSendError::WouldBlock,
             udp::SendError::Unaddressable => SocketSendError::InvalidDestination,
         })
}


/// UDP recvfrom。返回 (字节数, 来源IP, 来源端口)。
pub fn socket_recvfrom(handle : SocketHandle,
                       buf : &mut [u8])
                       -> Result<(usize, [u8; 4], u16), NetworkError> {
    use smoltcp::wire::IpAddress;
    {
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut()
                         .ok_or(NetworkError::StackUnavailable)?;
        if let Some(queue) = stack.udp_loopback
                                  .get_mut(&handle)
        {
            if let Some(packet) = queue.pop_front() {
                let n = packet.data
                              .len()
                              .min(buf.len());
                buf[..n].copy_from_slice(&packet.data[..n]);
                return Ok((n, packet.source_ip, packet.source_port));
            }
        }
    }
    with_udp_socket(handle, |s| s.recv_slice(buf)).ok_or(NetworkError::StackUnavailable)
                                                  .and_then(|r| r.map_err(|_| NetworkError::Io))
                                                  .map(|(n, meta)| {
                                                      let ip = match meta.endpoint.addr {
                                                          IpAddress::Ipv4(addr) => addr.octets(),
                                                      };
                                                      (n, ip, meta.endpoint.port)
                                                  })
}


/// UDP socket 是否有数据可读。
pub fn socket_udp_can_recv(handle : SocketHandle) -> Result<bool, NetworkError> {
    {
        let guard = NETWORK_STACK.lock();
        let stack = guard.as_ref()
                         .ok_or(NetworkError::StackUnavailable)?;
        if stack.udp_loopback
                .get(&handle)
                .is_some_and(|queue| !queue.is_empty())
        {
            return Ok(true);
        }
    }
    with_udp_socket(handle, |s| s.can_recv()).ok_or(NetworkError::StackUnavailable)
}
