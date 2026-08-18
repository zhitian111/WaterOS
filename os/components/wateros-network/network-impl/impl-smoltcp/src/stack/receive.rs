//! 两阶段 socket 接收事务：预留数据，复制完成后再提交消费。

use smoltcp::iface::SocketHandle;
use smoltcp::socket::{tcp, udp};
use smoltcp::wire::IpAddress;

use super::global::with_stack_mut;
use super::state::NetworkStack;
use super::types::{SocketKind, SocketRecvError, SocketRecvFinish};

/// 一次尚未消费的接收队列前缀；实际数据由上层 lease 持有。
pub struct SocketRecvReservation {
    /// 预留数据所属 socket。
    handle : SocketHandle,
    /// 与 `SocketMeta.recv_reservation` 比较的令牌，防止重复提交。
    id : u64,
    /// TCP 或 UDP；决定提交时如何消费底层队列。
    kind : SocketKind,
    /// 已复制到调用者缓冲区的字节数。
    staged_len : usize,
    /// 当前数据报完整长度；TCP 中等于本次可见字节数。
    datagram_len : usize,
    /// UDP 来源 IPv4 地址。
    source_ip : [u8; 4],
    /// UDP 来源端口。
    source_port : u16,
    /// 是否来自内核维护的本机回环 UDP 队列。
    loopback_udp : bool,
}

impl SocketRecvReservation {
    /// 返回已暂存到用户缓冲区的字节数。
    pub fn staged_len(&self) -> usize { self.staged_len }

    /// 返回 UDP 来源端点；TCP 返回连接对端端点。
    pub fn source(&self) -> ([u8; 4], u16) { (self.source_ip, self.source_port) }

    pub fn kind(&self) -> SocketKind { self.kind }

    pub fn datagram_len(&self) -> usize { self.datagram_len }
}

impl NetworkStack {
    fn prepare_recv(&mut self,
                    handle : SocketHandle,
                    buf : &mut [u8])
                    -> Result<SocketRecvReservation, SocketRecvError> {
        let (kind, id, peer_ip, peer_port) = {
            let meta = self.metas
                           .get_mut(&handle)
                           .ok_or(SocketRecvError::InvalidSocket)?;
            if meta.recv_reservation
                   .is_some()
            {
                return Err(SocketRecvError::Busy);
            }
            let id = meta.next_recv_reservation;
            meta.next_recv_reservation = meta.next_recv_reservation
                                             .wrapping_add(1);
            meta.recv_reservation = Some(id);
            (meta.kind, id, meta.peer_ip, meta.peer_port)
        };

        let prepared = match kind {
            SocketKind::Tcp => {
                let socket = self.sockets
                                 .get_mut::<tcp::Socket>(handle);
                let n = match socket.peek_slice(buf) {
                    Ok(n) => n,
                    Err(_) => {
                        if let Some(meta) = self.metas
                                                .get_mut(&handle)
                        {
                            meta.recv_reservation = None;
                        }
                        return Err(SocketRecvError::Io);
                    }
                };
                if n == 0 {
                    let may_recv = socket.may_recv();
                    if let Some(meta) = self.metas
                                            .get_mut(&handle)
                    {
                        meta.recv_reservation = None;
                    }
                    return if may_recv {
                        Err(SocketRecvError::Empty)
                    } else {
                        Err(SocketRecvError::Finished)
                    };
                }
                SocketRecvReservation { handle,
                                        id,
                                        kind,
                                        staged_len : n,
                                        datagram_len : n,
                                        source_ip : peer_ip,
                                        source_port : peer_port,
                                        loopback_udp : false }
            }
            SocketKind::Udp => {
                if let Some(packet) = self.udp_loopback
                                          .get(&handle)
                                          .and_then(|queue| queue.front())
                {
                    let n = packet.data
                                  .len()
                                  .min(buf.len());
                    buf[..n].copy_from_slice(&packet.data[..n]);
                    SocketRecvReservation { handle,
                                            id,
                                            kind,
                                            staged_len : n,
                                            datagram_len : packet.data.len(),
                                            source_ip : packet.source_ip,
                                            source_port : packet.source_port,
                                            loopback_udp : true }
                } else {
                    let socket = self.sockets
                                     .get_mut::<udp::Socket>(handle);
                    let (payload, metadata) = match socket.peek() {
                        Ok(value) => value,
                        Err(udp::RecvError::Exhausted) => {
                            if let Some(meta) = self.metas
                                                    .get_mut(&handle)
                            {
                                meta.recv_reservation = None;
                            }
                            return Err(SocketRecvError::Empty);
                        }
                        Err(_) => {
                            if let Some(meta) = self.metas
                                                    .get_mut(&handle)
                            {
                                meta.recv_reservation = None;
                            }
                            return Err(SocketRecvError::Io);
                        }
                    };
                    let n = payload.len()
                                   .min(buf.len());
                    buf[..n].copy_from_slice(&payload[..n]);
                    let source_ip = match metadata.endpoint
                                                  .addr
                    {
                        IpAddress::Ipv4(addr) => addr.octets(),
                    };
                    SocketRecvReservation { handle,
                                            id,
                                            kind,
                                            staged_len : n,
                                            datagram_len : payload.len(),
                                            source_ip,
                                            source_port : metadata.endpoint
                                                                  .port,
                                            loopback_udp : false }
                }
            }
        };
        Ok(prepared)
    }

    fn finish_recv(&mut self,
                   reservation : SocketRecvReservation,
                   copied : usize,
                   complete : bool)
                   -> Result<SocketRecvFinish, SocketRecvError> {
        if copied > reservation.staged_len {
            if let Some(meta) = self.metas
                                    .get_mut(&reservation.handle)
            {
                if meta.recv_reservation == Some(reservation.id) {
                    meta.recv_reservation = None;
                }
            }
            return Err(SocketRecvError::Io);
        }

        let active_matches = self.metas
                                 .get(&reservation.handle)
                                 .is_some_and(|meta| meta.recv_reservation == Some(reservation.id));
        if !active_matches {
            return Err(SocketRecvError::InvalidSocket);
        }

        if copied == 0 && !complete {
            if let Some(meta) = self.metas
                                    .get_mut(&reservation.handle)
            {
                meta.recv_reservation = None;
            }
            return Ok(SocketRecvFinish::Fault);
        }

        let consume_result = match reservation.kind {
            SocketKind::Tcp => {
                if copied == 0 {
                    Ok(())
                } else {
                    let socket = self.sockets
                                     .get_mut::<tcp::Socket>(reservation.handle);
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
            SocketKind::Udp if reservation.loopback_udp => self.udp_loopback
                                                               .get_mut(&reservation.handle)
                                                               .and_then(|queue| queue.pop_front())
                                                               .map(|_| ())
                                                               .ok_or(SocketRecvError::Io),
            SocketKind::Udp => self.sockets
                                   .get_mut::<udp::Socket>(reservation.handle)
                                   .recv()
                                   .map(|_| ())
                                   .map_err(|_| SocketRecvError::Io),
        };
        if let Some(meta) = self.metas
                                .get_mut(&reservation.handle)
        {
            meta.recv_reservation = None;
        }
        consume_result?;
        Ok(SocketRecvFinish::Bytes(copied))
    }
}

/// 预留接收队列前缀但暂不消费；用户复制完成后再调用 [`socket_finish_recv`]。
pub fn socket_prepare_recv(handle : SocketHandle,
                           buf : &mut [u8])
                           -> Result<SocketRecvReservation, SocketRecvError> {
    with_stack_mut(SocketRecvError::Io, |stack| {
        stack.prepare_recv(handle, buf)
    })
}

/// 提交已复制的前缀，或在立即 fault 时取消预留而不消费数据。
pub fn socket_finish_recv(reservation : SocketRecvReservation,
                          copied : usize,
                          complete : bool)
                          -> Result<SocketRecvFinish, SocketRecvError> {
    with_stack_mut(SocketRecvError::Io, |stack| {
        stack.finish_recv(reservation, copied, complete)
    })
}
