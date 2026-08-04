//! UDP socket 创建、数据报收发与本机回环队列。

use alloc::vec;
use smoltcp::iface::SocketHandle;
use smoltcp::socket::udp;

use super::global::with_stack_mut;
use super::socket::listen_endpoint;
use super::state::{
    LoopbackUdpQueue, NetworkStack, SocketMeta, UDP_MAX_PAYLOAD_SIZE, UDP_PACKET_DATA_SIZE,
    UDP_PACKET_METADATA_COUNT, UDP_USE_SMOLTCP_LOOPBACK,
};
use super::types::{NetworkError, SocketKind, SocketSendError, SocketState};

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

impl NetworkStack {
    fn create_udp_socket(&mut self) -> Result<SocketHandle, NetworkError> {
        let rx =
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; UDP_PACKET_METADATA_COUNT],
                                   vec![0; UDP_PACKET_DATA_SIZE]);
        let tx =
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; UDP_PACKET_METADATA_COUNT],
                                   vec![0; UDP_PACKET_DATA_SIZE]);
        let handle = self.sockets
                         .add(udp::Socket::new(rx, tx));
        self.metas
            .insert(handle, SocketMeta::new(SocketKind::Udp));
        Ok(handle)
    }

    pub(super) fn ensure_udp_bound(&mut self, handle : SocketHandle) -> Result<u16, NetworkError> {
        let (kind, state, local_ip) = {
            let meta = self.socket_meta(handle)?;
            (meta.kind, meta.state, meta.local_ip)
        };
        if kind != SocketKind::Udp {
            return Err(NetworkError::WrongSocketType);
        }
        match state {
            SocketState::Bound { port } => Ok(port),
            SocketState::Connected => {
                let port = self.socket_meta(handle)?
                               .local_port;
                if port == 0 {
                    Err(NetworkError::NotBound)
                } else {
                    Ok(port)
                }
            }
            SocketState::Created => {
                let local_port = self.next_ephemeral_port();
                self.sockets
                    .get_mut::<udp::Socket>(handle)
                    .bind(listen_endpoint(local_ip, local_port))
                    .map_err(|_| NetworkError::AddressInUse)?;
                let meta = self.socket_meta_mut(handle)?;
                meta.state = SocketState::Bound { port : local_port };
                meta.local_port = local_port;
                Ok(local_port)
            }
            _ => Err(NetworkError::NotBound),
        }
    }

    fn deliver_loopback_udp(&mut self,
                            source_port : u16,
                            dest_ip : [u8; 4],
                            dest_port : u16,
                            data : &[u8]) {
        let source_ip = if dest_ip[0] == 127 {
            [127, 0, 0, 1]
        } else {
            self.local_ip
        };
        let connected_target =
            self.metas
                .iter()
                .find_map(|(&handle, meta)| {
                    if meta.kind != SocketKind::Udp {
                        return None;
                    }
                    match meta.state {
                        SocketState::Connected
                            if meta.local_port == dest_port &&
                               local_addr_matches(meta.local_ip, dest_ip, self.local_ip) &&
                               meta.peer_port == source_port &&
                               (meta.peer_ip == source_ip ||
                                (meta.peer_ip[0] == 127 && source_ip[0] == 127)) =>
                        {
                            Some(handle)
                        }
                        _ => None,
                    }
                });
        let target = connected_target.or_else(|| {
                                         self.metas
                                             .iter()
                                             .find_map(|(&handle, meta)| {
                                                 if meta.kind != SocketKind::Udp {
                                                     return None;
                                                 }
                                                 match meta.state {
                                                     SocketState::Bound { port }
                                                         if port == dest_port &&
                                                            local_addr_matches(meta.local_ip,
                                                                               dest_ip,
                                                                               self.local_ip) =>
                                                     {
                                                         Some(handle)
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
        let queue = self.udp_loopback
                        .entry(target)
                        .or_insert_with(LoopbackUdpQueue::default);
        let _delivered = queue.try_push(data, source_ip, source_port);
    }

    pub(super) fn send_udp_to(&mut self,
                              handle : SocketHandle,
                              data : &[u8],
                              ip : [u8; 4],
                              port : u16)
                              -> Result<usize, SocketSendError> {
        use smoltcp::wire::IpAddress;

        if data.len() > UDP_MAX_PAYLOAD_SIZE {
            return Err(SocketSendError::MessageTooLarge);
        }
        let source_port = self.ensure_udp_bound(handle)
                              .map_err(|error| match error {
                                  NetworkError::InvalidSocket | NetworkError::WrongSocketType => {
                                      SocketSendError::InvalidSocket
                                  }
                                  NetworkError::NotBound => SocketSendError::NotConnected,
                                  _ => SocketSendError::Io,
                              })?;
        if !UDP_USE_SMOLTCP_LOOPBACK && is_local_destination(ip, self.local_ip) {
            self.deliver_loopback_udp(source_port, ip, port, data);
            return Ok(data.len());
        }
        self.sockets
            .get_mut::<udp::Socket>(handle)
            .send_slice(data,
                        (IpAddress::v4(ip[0], ip[1], ip[2], ip[3]), port))
            .map(|()| data.len())
            .map_err(|error| match error {
                udp::SendError::BufferFull => SocketSendError::WouldBlock,
                udp::SendError::Unaddressable => SocketSendError::InvalidDestination,
            })
    }
}

/// 创建 UDP socket，返回其 smoltcp 句柄。
pub fn create_udp_socket() -> Result<SocketHandle, NetworkError> {
    with_stack_mut(NetworkError::StackUnavailable,
                   NetworkStack::create_udp_socket)
}

/// UDP sendto。
pub fn socket_sendto(handle : SocketHandle,
                     data : &[u8],
                     ip : [u8; 4],
                     port : u16)
                     -> Result<usize, SocketSendError> {
    with_stack_mut(SocketSendError::StackUnavailable,
                   |stack| stack.send_udp_to(handle, data, ip, port))
}
