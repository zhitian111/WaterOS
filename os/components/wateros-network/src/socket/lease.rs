//! Socket 接收租约：先预留协议栈数据，用户复制完成后再提交消费。

use alloc::vec::Vec;

use crate::stack;
use crate::{SocketKind, SocketRecvError, SocketRecvFinish};

use super::SocketRef;

impl SocketRef {
    /// 预留接收数据并保持 socket 生命周期，直到用户复制提交或取消。
    pub fn prepare_receive(&self, max_len : usize) -> Result<SocketReceiveLease, SocketRecvError> {
        let snapshot = self.poll_snapshot()
                           .map_err(|_| SocketRecvError::InvalidSocket)?;
        if !snapshot.can_recv {
            if snapshot.kind == SocketKind::Tcp && !snapshot.may_recv {
                return Err(SocketRecvError::Finished);
            }
            return Err(SocketRecvError::Empty);
        }
        let mut data = Vec::new();
        data.try_reserve_exact(max_len)
            .map_err(|_| SocketRecvError::NoMemory)?;
        data.resize(max_len, 0);
        let reservation =
            self.with_handle(|handle| stack::socket_prepare_recv(handle, &mut data))?;
        data.truncate(reservation.staged_len());
        Ok(SocketReceiveLease { _socket : self.clone(),
                                reservation : Some(reservation),
                                data })
    }
}

/// 供 read、recvfrom 和 recvmsg 共享的已预留接收数据。
pub struct SocketReceiveLease {
    _socket : SocketRef,
    reservation : Option<stack::SocketRecvReservation>,
    data : Vec<u8>,
}

impl SocketReceiveLease {
    pub fn bytes(&self) -> &[u8] { self.data.as_slice() }

    pub fn source(&self) -> ([u8; 4], u16) {
        self.reservation
            .as_ref()
            .map(stack::SocketRecvReservation::source)
            .unwrap_or(([0; 4], 0))
    }

    pub fn kind(&self) -> SocketKind {
        self.reservation
            .as_ref()
            .map(stack::SocketRecvReservation::kind)
            .unwrap_or(SocketKind::Tcp)
    }

    pub fn datagram_len(&self) -> usize {
        self.reservation
            .as_ref()
            .map(stack::SocketRecvReservation::datagram_len)
            .unwrap_or(0)
    }

    pub fn finish(mut self,
                  copied : usize,
                  complete : bool)
                  -> Result<SocketRecvFinish, SocketRecvError> {
        let reservation = self.reservation
                              .take()
                              .ok_or(SocketRecvError::Io)?;
        stack::socket_finish_recv(reservation, copied, complete)
    }
}

impl Drop for SocketReceiveLease {
    fn drop(&mut self) {
        if let Some(reservation) = self.reservation
                                       .take()
        {
            let _ = stack::socket_finish_recv(reservation, 0, false);
        }
    }
}
