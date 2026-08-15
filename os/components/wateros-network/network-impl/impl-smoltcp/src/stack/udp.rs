//! UDP socket 创建、数据报收发与本机回环队列。

use alloc::vec;
use smoltcp::iface::SocketHandle;
use smoltcp::socket::udp;

use super::global::with_stack_mut;
#[cfg(feature = "ipv6")]
use super::socket::loopback_address;
use super::socket::{listen_endpoint, smoltcp_ip};
use super::state::{
    LoopbackUdpQueue, NetworkStack, SocketMeta, UDP6_MAX_PAYLOAD_SIZE, UDP_MAX_PAYLOAD_SIZE,
    UDP_PACKET_DATA_SIZE, UDP_PACKET_METADATA_COUNT, UDP_USE_SMOLTCP_LOOPBACK,
};
use super::types::{
    NetworkAddress, NetworkError, SocketDomain, SocketKind, SocketSendError, SocketState,
};

fn is_local_destination(ip : NetworkAddress, stack : &NetworkStack) -> bool {
    ip.is_loopback() || stack.configured_address(ip.domain()) == Some(ip)
}

fn local_addr_matches(bound : Option<NetworkAddress>,
                      dest : NetworkAddress,
                      configured : Option<NetworkAddress>)
                      -> bool {
    match bound {
        None => true,
        Some(ip) if ip.is_loopback() && dest.is_loopback() && ip.domain() == dest.domain() => true,
        Some(ip) => ip == dest || (dest.is_loopback() && Some(ip) == configured),
    }
}

impl NetworkStack {
    pub(super) fn udp_bind_conflicts(&self,
                                     handle : SocketHandle,
                                     domain : SocketDomain,
                                     local_ip : Option<NetworkAddress>,
                                     port : u16)
                                     -> Result<bool, NetworkError> {
        let requested = self.socket_meta(handle)?;
        Ok(self.metas
               .iter()
               .any(|(&other_handle, other)| {
                   if other_handle == handle || other.kind != SocketKind::Udp ||
                      other.domain != domain || other.local_port != port ||
                      !matches!(other.state, SocketState::Bound { .. } | SocketState::Connected)
                   {
                       return false;
                   }
                   let addresses_overlap = local_ip.is_none() || other.local_ip.is_none() ||
                                           local_ip == other.local_ip;
                   let sharing_allowed = (requested.reuse_port && other.reuse_port) ||
                                         (requested.reuse_addr && other.reuse_addr);
                   addresses_overlap && !sharing_allowed
               }))
    }

    pub(super) fn next_udp_ephemeral_port(&mut self,
                                          handle : SocketHandle,
                                          domain : SocketDomain,
                                          local_ip : Option<NetworkAddress>)
                                          -> Result<u16, NetworkError> {
        for _ in 0..=(u16::MAX - 49152) {
            let port = self.next_ephemeral_port();
            if !self.udp_bind_conflicts(handle, domain, local_ip, port)? {
                return Ok(port);
            }
        }
        Err(NetworkError::AddressInUse)
    }

    fn create_udp_socket(&mut self, domain : SocketDomain) -> Result<SocketHandle, NetworkError> {
        #[cfg(not(feature = "ipv6"))]
        if domain == SocketDomain::Ipv6 {
            return Err(NetworkError::Unsupported);
        }
        let rx =
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; UDP_PACKET_METADATA_COUNT],
                                   vec![0; UDP_PACKET_DATA_SIZE]);
        let tx =
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; UDP_PACKET_METADATA_COUNT],
                                   vec![0; UDP_PACKET_DATA_SIZE]);
        let handle = self.sockets
                         .add(udp::Socket::new(rx, tx));
        self.metas
            .insert(handle,
                    SocketMeta::new(domain, SocketKind::Udp));
        Ok(handle)
    }

    pub(super) fn ensure_udp_bound(&mut self, handle : SocketHandle) -> Result<u16, NetworkError> {
        let (domain, kind, state, local_ip) = {
            let meta = self.socket_meta(handle)?;
            (meta.domain, meta.kind, meta.state, meta.local_ip)
        };
        #[cfg(not(feature = "ipv6"))]
        let _ = domain;
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
                let local_port = self.next_udp_ephemeral_port(handle, domain, local_ip)?;
                #[cfg(feature = "ipv6")]
                let bind_ip = Some(local_ip.or_else(|| self.configured_address(domain))
                                           .unwrap_or_else(|| loopback_address(domain)));
                #[cfg(not(feature = "ipv6"))]
                let bind_ip = local_ip;
                self.sockets
                    .get_mut::<udp::Socket>(handle)
                    .bind(listen_endpoint(bind_ip, local_port))
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
                            dest_ip : NetworkAddress,
                            dest_port : u16,
                            data : &[u8]) {
        let configured = self.configured_address(dest_ip.domain());
        let source_ip = if dest_ip.is_loopback() {
            dest_ip
        } else {
            configured.unwrap_or_else(|| NetworkAddress::unspecified(dest_ip.domain()))
        };
        let connected_target =
            self.metas
                .iter()
                .find_map(|(&handle, meta)| {
                    if meta.kind != SocketKind::Udp || meta.domain != dest_ip.domain() {
                        return None;
                    }
                    match meta.state {
                        SocketState::Connected
                            if meta.local_port == dest_port &&
                               local_addr_matches(meta.local_ip, dest_ip, configured) &&
                               meta.peer_port == source_port &&
                               (meta.peer_ip == source_ip ||
                                (meta.peer_ip
                                     .is_loopback() &&
                                 source_ip.is_loopback() &&
                                 meta.peer_ip
                                     .domain() ==
                                 source_ip.domain())) =>
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
                                                 if meta.kind != SocketKind::Udp ||
                                                    meta.domain != dest_ip.domain()
                                                 {
                                                     return None;
                                                 }
                                                 match meta.state {
                                                     SocketState::Bound { port }
                                                         if port == dest_port &&
                                                            local_addr_matches(meta.local_ip,
                                                                               dest_ip,
                                                                               configured) =>
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
        let _delivered = queue.try_push(data, source_ip, source_port, dest_ip);
    }

    pub(super) fn send_udp_to(&mut self,
                              handle : SocketHandle,
                              data : &[u8],
                              ip : NetworkAddress,
                              port : u16)
                              -> Result<usize, SocketSendError> {
        let domain = self.socket_meta(handle)
                         .map_err(|_| SocketSendError::InvalidSocket)?
                         .domain;
        if ip.domain() != domain {
            return Err(SocketSendError::InvalidDestination);
        }
        let max_payload = match domain {
            SocketDomain::Ipv4 => UDP_MAX_PAYLOAD_SIZE,
            SocketDomain::Ipv6 => UDP6_MAX_PAYLOAD_SIZE,
        };
        if data.len() > max_payload {
            return Err(SocketSendError::MessageTooLarge);
        }
        // 当前没有启用 IP 分片。发往真实链路的 UDP 数据报若超过设备 MTU，
        // 必须同步返回 EMSGSIZE，不能先报告成功再由 smoltcp 静默丢弃。
        if !is_local_destination(ip, self) {
            let ip_header_len = match domain {
                SocketDomain::Ipv4 => 20,
                SocketDomain::Ipv6 => 40,
            };
            let mtu_payload = self.adapter
                                  .ip_mtu()
                                  .saturating_sub(ip_header_len + 8);
            if data.len() > mtu_payload {
                return Err(SocketSendError::MessageTooLarge);
            }
        }
        let source_port = self.ensure_udp_bound(handle)
                              .map_err(|error| match error {
                                  NetworkError::InvalidSocket | NetworkError::WrongSocketType => {
                                      SocketSendError::InvalidSocket
                                  }
                                  NetworkError::NotBound => SocketSendError::NotConnected,
                                  _ => SocketSendError::Io,
                              })?;
        if !UDP_USE_SMOLTCP_LOOPBACK && is_local_destination(ip, self) {
            self.deliver_loopback_udp(source_port, ip, port, data);
            return Ok(data.len());
        }
        self.sockets
            .get_mut::<udp::Socket>(handle)
            .send_slice(data, (smoltcp_ip(ip), port))
            .map(|()| data.len())
            .map_err(|error| match error {
                udp::SendError::BufferFull => SocketSendError::WouldBlock,
                udp::SendError::Unaddressable => SocketSendError::InvalidDestination,
            })
    }
}

/// 创建 UDP socket，返回其 smoltcp 句柄。
pub fn create_udp_socket(domain : SocketDomain) -> Result<SocketHandle, NetworkError> {
    with_stack_mut(NetworkError::StackUnavailable,
                   |stack| stack.create_udp_socket(domain))
}
