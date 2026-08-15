//! Echo-only raw ICMP/ICMPv6 socket support used by ping and ping6.

use alloc::vec;
use alloc::vec::Vec;
use smoltcp::iface::SocketHandle;
use smoltcp::socket::icmp;

use super::global::with_stack_mut;
use super::socket::{network_address, smoltcp_ip};
use super::state::{
    NetworkStack, PendingIcmpPacket, SocketMeta, ICMP_PACKET_DATA_SIZE,
    ICMP_PACKET_METADATA_COUNT,
};
use super::types::{
    NetworkAddress, NetworkError, SocketDomain, SocketKind, SocketSendError,
};

const ICMP_HEADER_LEN : usize = 8;
const ICMPV4_ECHO_REPLY : u8 = 0;
const ICMPV4_ECHO_REQUEST : u8 = 8;
const ICMPV6_ECHO_REQUEST : u8 = 128;
const ICMPV6_ECHO_REPLY : u8 = 129;
const IPV4_HEADER_LEN : usize = 20;

impl NetworkStack {
    fn create_icmp_socket(&mut self,
                          domain : SocketDomain)
                          -> Result<SocketHandle, NetworkError> {
        #[cfg(not(feature = "ipv6"))]
        if domain == SocketDomain::Ipv6 {
            return Err(NetworkError::Unsupported);
        }
        let rx = icmp::PacketBuffer::new(
            vec![icmp::PacketMetadata::EMPTY; ICMP_PACKET_METADATA_COUNT],
            vec![0; ICMP_PACKET_DATA_SIZE],
        );
        let tx = icmp::PacketBuffer::new(
            vec![icmp::PacketMetadata::EMPTY; ICMP_PACKET_METADATA_COUNT],
            vec![0; ICMP_PACKET_DATA_SIZE],
        );
        let handle = self.sockets.add(icmp::Socket::new(rx, tx));
        self.metas.insert(handle, SocketMeta::new(domain, SocketKind::Icmp));
        Ok(handle)
    }

    pub(super) fn send_icmp_to(&mut self,
                               handle : SocketHandle,
                               data : &[u8],
                               ip : NetworkAddress)
                               -> Result<usize, SocketSendError> {
        let (domain, kind, bound_ident) = self.metas
                                              .get(&handle)
                                              .map(|meta| {
                                                  (meta.domain,
                                                   meta.kind,
                                                   meta.icmp_ident)
                                              })
                                              .ok_or(SocketSendError::InvalidSocket)?;
        if kind != SocketKind::Icmp || ip.domain() != domain || ip.is_unspecified() {
            return Err(SocketSendError::InvalidDestination);
        }
        if data.len() < ICMP_HEADER_LEN || data.len() > icmp_payload_limit(domain) {
            return Err(SocketSendError::MessageTooLarge);
        }
        let valid_echo_type = match domain {
            SocketDomain::Ipv4 => matches!(data[0], ICMPV4_ECHO_REQUEST | ICMPV4_ECHO_REPLY),
            SocketDomain::Ipv6 => matches!(data[0], ICMPV6_ECHO_REQUEST | ICMPV6_ECHO_REPLY),
        };
        if !valid_echo_type || data[1] != 0 {
            return Err(SocketSendError::InvalidDestination);
        }
        let ident = u16::from_be_bytes([data[4], data[5]]);
        if bound_ident.is_some_and(|bound| bound != ident) {
            return Err(SocketSendError::InvalidDestination);
        }

        let socket = self.sockets.get_mut::<icmp::Socket>(handle);
        if bound_ident.is_none() {
            socket.bind(icmp::Endpoint::Ident(ident))
                  .map_err(|_| SocketSendError::Io)?;
            self.metas.get_mut(&handle)
                      .ok_or(SocketSendError::InvalidSocket)?
                      .icmp_ident = Some(ident);
        }
        socket.send_slice(data, smoltcp_ip(ip))
              .map(|()| data.len())
              .map_err(|error| match error {
                  icmp::SendError::BufferFull => SocketSendError::WouldBlock,
                  icmp::SendError::Unaddressable => SocketSendError::InvalidDestination,
              })
    }

    /// smoltcp ICMP receive 会立即出队；转存完整报文以实现 WaterOS 的两阶段
    /// 接收事务。IPv4 raw socket 按 Linux ABI 补一个最小 IPv4 头。
    pub(super) fn ensure_icmp_pending(&mut self,
                                      handle : SocketHandle)
                                      -> Result<(), super::types::SocketRecvError> {
        if self.icmp_pending.contains_key(&handle) {
            return Ok(());
        }
        let (domain, local_ip) = self.metas
                                     .get(&handle)
                                     .map(|meta| (meta.domain, meta.local_ip))
                                     .ok_or(super::types::SocketRecvError::InvalidSocket)?;
        let (payload, source) = self.sockets
                                    .get_mut::<icmp::Socket>(handle)
                                    .recv()
                                    .map_err(|error| match error {
                                        icmp::RecvError::Exhausted => {
                                            super::types::SocketRecvError::Empty
                                        }
                                        icmp::RecvError::Truncated => {
                                            super::types::SocketRecvError::Io
                                        }
                                    })?;
        let source_ip = network_address(source);
        let data = match domain {
            SocketDomain::Ipv4 => {
                let destination = local_ip.unwrap_or_else(|| {
                    if source_ip.is_loopback() {
                        NetworkAddress::Ipv4([127, 0, 0, 1])
                    } else {
                        NetworkAddress::Ipv4(self.local_ipv4)
                    }
                });
                ipv4_raw_packet(source_ip, destination, payload)
            }
            SocketDomain::Ipv6 => payload.to_vec(),
        };
        self.icmp_pending.insert(handle, PendingIcmpPacket { data, source_ip });
        Ok(())
    }
}

fn icmp_payload_limit(domain : SocketDomain) -> usize {
    match domain {
        SocketDomain::Ipv4 => u16::MAX as usize - IPV4_HEADER_LEN,
        SocketDomain::Ipv6 => u16::MAX as usize,
    }
}

fn ipv4_raw_packet(source : NetworkAddress,
                   destination : NetworkAddress,
                   payload : &[u8])
                   -> Vec<u8> {
    let NetworkAddress::Ipv4(source) = source else {
        return payload.to_vec();
    };
    let NetworkAddress::Ipv4(destination) = destination else {
        return payload.to_vec();
    };
    let total_len = IPV4_HEADER_LEN + payload.len();
    let mut packet = vec![0u8; total_len];
    packet[0] = 0x45;
    packet[2..4].copy_from_slice(&(total_len as u16).to_be_bytes());
    packet[8] = 64;
    packet[9] = 1;
    packet[12..16].copy_from_slice(&source);
    packet[16..20].copy_from_slice(&destination);
    let checksum = internet_checksum(&packet[..IPV4_HEADER_LEN]);
    packet[10..12].copy_from_slice(&checksum.to_be_bytes());
    packet[IPV4_HEADER_LEN..].copy_from_slice(payload);
    packet
}

fn internet_checksum(bytes : &[u8]) -> u16 {
    let mut sum = 0u32;
    for pair in bytes.chunks(2) {
        let word = if pair.len() == 2 {
            u16::from_be_bytes([pair[0], pair[1]])
        } else {
            u16::from(pair[0]) << 8
        };
        sum += u32::from(word);
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

pub fn create_icmp_socket(domain : SocketDomain) -> Result<SocketHandle, NetworkError> {
    with_stack_mut(NetworkError::StackUnavailable,
                   |stack| stack.create_icmp_socket(domain))
}

/// Echo-only ICMP/ICMPv6 sendto. sockaddr 的端口字段对 raw ICMP 无意义。
pub fn socket_sendto(handle : SocketHandle,
                     data : &[u8],
                     ip : NetworkAddress,
                     port : u16)
                     -> Result<usize, SocketSendError> {
    with_stack_mut(SocketSendError::StackUnavailable, |stack| {
        let kind = stack.socket_meta(handle)
                        .map_err(|_| SocketSendError::InvalidSocket)?
                        .kind;
        match kind {
            SocketKind::Udp => stack.send_udp_to(handle, data, ip, port),
            SocketKind::Icmp => stack.send_icmp_to(handle, data, ip),
            SocketKind::Tcp => Err(SocketSendError::InvalidDestination),
        }
    })
}
