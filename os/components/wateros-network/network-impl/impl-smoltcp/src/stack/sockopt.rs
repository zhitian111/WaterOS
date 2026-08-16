//! Linux socket 选项与 smoltcp 状态之间的转换。

use alloc::vec;
use alloc::vec::Vec;
use smoltcp::iface::SocketHandle;
use smoltcp::socket::tcp;
use smoltcp::time::Duration;

use super::global::{with_stack, with_stack_mut};
use super::poll::poll_socket_events;
use super::state::{NetworkStack, SocketMeta, TCP_MSS, TCP_RX_BUFFER_SIZE, TCP_TX_BUFFER_SIZE};
use super::tcp::tcp_is_connected;
use super::types::{NetworkError, SocketConnectError, SocketKind};

const SOL_IP : usize = 0;
const IPPROTO_IP : usize = 0;
const SOL_SOCKET : usize = 1;
const IPPROTO_TCP : usize = 6;
const IP_TOS : usize = 1;
const SO_REUSEADDR : usize = 2;
const SO_ERROR : usize = 4;
const SO_DONTROUTE : usize = 5;
const SO_SNDBUF : usize = 7;
const SO_RCVBUF : usize = 8;
const SO_KEEPALIVE : usize = 9;
const SO_REUSEPORT : usize = 15;
const SO_RCVTIMEO_OLD : usize = 20;
const SO_SNDTIMEO_OLD : usize = 21;
const IP_ADD_MEMBERSHIP : usize = 35;
const IP_DROP_MEMBERSHIP : usize = 36;
const MCAST_JOIN_GROUP : usize = 42;
const MCAST_LEAVE_GROUP : usize = 45;
const SO_RCVTIMEO_NEW : usize = 66;
const SO_SNDTIMEO_NEW : usize = 67;
const IP_RECVERR : usize = 11;
const TCP_NODELAY : usize = 1;
const TCP_MAXSEG : usize = 2;
const TCP_INFO : usize = 11;
const ETIMEDOUT : i32 = 110;
const ECONNREFUSED : i32 = 111;
/// Linux 未调整 keepalive 参数时使用的默认空闲时间。
const TCP_KEEPALIVE_DEFAULT_SECS : u64 = 2 * 60 * 60;

fn timeval_to_millis(optval : &[u8]) -> Result<Option<u64>, NetworkError> {
    if optval.len() >= 16 {
        let mut sec = [0u8; 8];
        let mut usec = [0u8; 8];
        sec.copy_from_slice(&optval[0..8]);
        usec.copy_from_slice(&optval[8..16]);
        let sec = i64::from_ne_bytes(sec);
        let usec = i64::from_ne_bytes(usec);
        if sec < 0 || usec < 0 || usec >= 1_000_000 {
            return Err(NetworkError::InvalidArgument);
        }
        if sec == 0 && usec == 0 {
            return Ok(None);
        }
        let millis = (sec as u64).saturating_mul(1000)
                                 .saturating_add(((usec as u64).saturating_add(999)) / 1000)
                                 .max(1);
        return Ok(Some(millis));
    }
    if optval.len() >= 8 {
        let mut sec = [0u8; 4];
        let mut usec = [0u8; 4];
        sec.copy_from_slice(&optval[0..4]);
        usec.copy_from_slice(&optval[4..8]);
        let sec = i32::from_ne_bytes(sec);
        let usec = i32::from_ne_bytes(usec);
        if sec < 0 || usec < 0 || usec >= 1_000_000 {
            return Err(NetworkError::InvalidArgument);
        }
        if sec == 0 && usec == 0 {
            return Ok(None);
        }
        let millis = (sec as u64).saturating_mul(1000)
                                 .saturating_add(((usec as u64).saturating_add(999)) / 1000)
                                 .max(1);
        return Ok(Some(millis));
    }
    Err(NetworkError::InvalidArgument)
}

fn millis_to_timeval(timeout_ms : Option<u64>) -> Vec<u8> {
    let millis = timeout_ms.unwrap_or(0);
    let sec = (millis / 1000) as i64;
    let usec = ((millis % 1000) * 1000) as i64;
    let mut out = Vec::with_capacity(16);
    out.extend_from_slice(&sec.to_ne_bytes());
    out.extend_from_slice(&usec.to_ne_bytes());
    out
}

fn sockopt_bool(optval : &[u8]) -> Result<bool, NetworkError> {
    if optval.is_empty() {
        return Err(NetworkError::InvalidArgument);
    }
    if optval.len() >= 4 {
        let mut raw = [0u8; 4];
        raw.copy_from_slice(&optval[..4]);
        return Ok(i32::from_ne_bytes(raw) != 0);
    }
    Ok(optval.iter()
             .any(|&byte| byte != 0))
}

fn sockopt_i32(optval : &[u8]) -> Result<i32, NetworkError> {
    if optval.len() < 4 {
        return Err(NetworkError::InvalidArgument);
    }
    let mut raw = [0u8; 4];
    raw.copy_from_slice(&optval[..4]);
    Ok(i32::from_ne_bytes(raw))
}

fn parse_ipv4_mcast_group(optval : &[u8]) -> Result<u32, NetworkError> {
    if optval.len() >= 16 {
        let family = u16::from_ne_bytes([optval[8], optval[9]]);
        if family == 2 {
            return Ok(u32::from_ne_bytes([optval[12],
                                          optval[13],
                                          optval[14],
                                          optval[15]]));
        }
    }
    if optval.len() >= 12 {
        let family = u16::from_ne_bytes([optval[4], optval[5]]);
        if family == 2 {
            return Ok(u32::from_ne_bytes([optval[8],
                                          optval[9],
                                          optval[10],
                                          optval[11]]));
        }
    }
    if optval.len() >= 8 {
        return Ok(u32::from_ne_bytes([optval[0],
                                      optval[1],
                                      optval[2],
                                      optval[3]]));
    }
    Err(NetworkError::InvalidArgument)
}

fn mcast_join(meta : &mut SocketMeta, group : u32) {
    meta.mcast_groups
        .insert(group);
}

fn mcast_leave(meta : &mut SocketMeta, group : u32) -> Result<(), NetworkError> {
    if meta.mcast_groups
           .remove(&group)
    {
        Ok(())
    } else {
        Err(NetworkError::AddressNotAvailable)
    }
}

fn write_u32(buf : &mut [u8], offset : usize, value : u32) {
    if offset + 4 <= buf.len() {
        buf[offset..offset + 4].copy_from_slice(&value.to_ne_bytes());
    }
}

impl NetworkStack {
    /// 返回 true 表示修改后需要立即驱动一次 socket 状态更新。
    fn set_sockopt(&mut self,
                   handle : SocketHandle,
                   level : usize,
                   optname : usize,
                   optval : &[u8])
                   -> Result<bool, NetworkError> {
        if (level == SOL_IP || level == IPPROTO_IP) && optname == IP_TOS {
            // OpenSSH 会根据会话阶段设置 IP_TOS。smoltcp 暂无逐 socket 的
            // IPv4 TOS 接口，因此校验参数后兼容接受，不改变实际报文优先级。
            let _tos = sockopt_i32(optval)?;
            return Ok(false);
        }
        if (level == SOL_IP || level == IPPROTO_IP) &&
           matches!(optname,
                    IP_ADD_MEMBERSHIP | MCAST_JOIN_GROUP)
        {
            let group = parse_ipv4_mcast_group(optval)?;
            mcast_join(self.socket_meta_mut(handle)?, group);
            return Ok(false);
        }
        if (level == SOL_IP || level == IPPROTO_IP) &&
           matches!(optname,
                    IP_DROP_MEMBERSHIP | MCAST_LEAVE_GROUP)
        {
            let group = parse_ipv4_mcast_group(optval)?;
            mcast_leave(self.socket_meta_mut(handle)?, group)?;
            return Ok(false);
        }
        if (level == SOL_IP || level == IPPROTO_IP) && optname == IP_RECVERR {
            // glibc 的 DNS 解析器会先启用 IP_RECVERR，失败时不会继续使用该 UDP
            // 套接字发送查询。smoltcp 尚未实现 Linux 错误队列，因此这里只校验参数
            // 并兼容性地返回成功，保持原有 UDP 收发行为不变。
            let _enabled = sockopt_bool(optval)?;
            return Ok(false);
        }
        if level == SOL_SOCKET && matches!(optname, SO_REUSEADDR | SO_REUSEPORT) {
            return Ok(false);
        }
        if level == SOL_SOCKET && optname == SO_DONTROUTE {
            let _enabled = sockopt_bool(optval)?;
            return Ok(false);
        }
        if level == SOL_SOCKET && optname == SO_KEEPALIVE {
            let enabled = sockopt_bool(optval)?;
            if self.socket_meta(handle)?
                   .kind !=
               SocketKind::Tcp
            {
                return Err(NetworkError::WrongSocketType);
            }
            self.sockets
                .get_mut::<tcp::Socket>(handle)
                .set_keep_alive(if enabled {
                                    Some(Duration::from_secs(TCP_KEEPALIVE_DEFAULT_SECS))
                                } else {
                                    None
                                });
            return Ok(false);
        }
        if level == SOL_SOCKET && optname == SO_SNDBUF {
            self.socket_meta_mut(handle)?
                .snd_buf_size = sockopt_i32(optval)?.max(0);
            return Ok(false);
        }
        if level == SOL_SOCKET && optname == SO_RCVBUF {
            self.socket_meta_mut(handle)?
                .rcv_buf_size = sockopt_i32(optval)?.max(0);
            return Ok(false);
        }
        if level == SOL_SOCKET &&
           matches!(optname,
                    SO_RCVTIMEO_OLD | SO_RCVTIMEO_NEW)
        {
            self.socket_meta_mut(handle)?
                .recv_timeout_ms = timeval_to_millis(optval)?;
            return Ok(false);
        }
        if level == SOL_SOCKET &&
           matches!(optname,
                    SO_SNDTIMEO_OLD | SO_SNDTIMEO_NEW)
        {
            let _timeout_ms = timeval_to_millis(optval)?;
            return Ok(false);
        }
        if level == IPPROTO_TCP && optname == TCP_NODELAY {
            let enabled = sockopt_bool(optval)?;
            if self.socket_meta(handle)?
                   .kind !=
               SocketKind::Tcp
            {
                return Err(NetworkError::WrongSocketType);
            }
            let socket = self.sockets
                             .get_mut::<tcp::Socket>(handle);
            socket.set_nagle_enabled(!enabled);
            socket.set_ack_delay(if enabled {
                                     None
                                 } else {
                                     Some(Duration::from_millis(10))
                                 });
            self.socket_meta_mut(handle)?
                .tcp_nodelay = enabled;
            return Ok(true);
        }
        Err(NetworkError::Unsupported)
    }

    fn recv_timeout_ms(&self, handle : SocketHandle) -> Result<Option<u64>, NetworkError> {
        Ok(self.socket_meta(handle)?
               .recv_timeout_ms)
    }

    fn tcp_info(&mut self, handle : SocketHandle) -> Vec<u8> {
        const TCP_INFO_LEN : usize = 256;
        const TCP_ESTABLISHED : u8 = 1;
        const TCP_CLOSE : u8 = 7;

        let mut out = vec![0u8; TCP_INFO_LEN];
        let is_tcp = self.metas
                         .get(&handle)
                         .is_some_and(|meta| meta.kind == SocketKind::Tcp);
        let connected = is_tcp &&
                        tcp_is_connected(self.sockets
                                             .get_mut::<tcp::Socket>(handle));
        out[0] = if connected {
            TCP_ESTABLISHED
        } else {
            TCP_CLOSE
        };
        let rcv_space = self.metas
                            .get(&handle)
                            .map(|meta| {
                                meta.rcv_buf_size
                                    .max(0) as u32
                            })
                            .unwrap_or(TCP_RX_BUFFER_SIZE as u32);
        let send_capacity = self.send_capacity(handle)
                                .unwrap_or(0) as u32;
        let cwnd_segments = (send_capacity / TCP_MSS).clamp(2, 64);

        // Linux uapi struct tcp_info offsets used by iperf3.
        write_u32(&mut out, 8, 200_000);
        write_u32(&mut out, 16, TCP_MSS);
        write_u32(&mut out, 20, TCP_MSS);
        write_u32(&mut out, 60, 1500);
        write_u32(&mut out, 64, TCP_TX_BUFFER_SIZE as u32);
        write_u32(&mut out, 68, 1_000);
        write_u32(&mut out, 72, 250);
        write_u32(&mut out, 76, u32::MAX);
        write_u32(&mut out, 80, cwnd_segments);
        write_u32(&mut out, 84, TCP_MSS);
        write_u32(&mut out, 88, 3);
        write_u32(&mut out, 96, rcv_space);
        write_u32(&mut out, 100, 0);
        write_u32(&mut out, 228, rcv_space);
        out
    }

    fn get_sockopt(&mut self,
                   handle : SocketHandle,
                   level : usize,
                   optname : usize)
                   -> Result<Vec<u8>, NetworkError> {
        if level == SOL_SOCKET && optname == SO_ERROR {
            // Linux 的 SO_ERROR 会取出并清除待处理错误；连接仍在进行或已经成功时为 0。
            let error = self.socket_meta_mut(handle)?
                            .connect_error
                            .take();
            let errno = match error {
                Some(SocketConnectError::TimedOut) => ETIMEDOUT,
                Some(SocketConnectError::ConnectionRefused) => ECONNREFUSED,
                None => 0,
            };
            return Ok(errno.to_ne_bytes()
                           .to_vec());
        }
        if level == SOL_SOCKET && optname == SO_SNDBUF {
            return Ok(self.socket_meta(handle)?
                          .snd_buf_size
                          .to_ne_bytes()
                          .to_vec());
        }
        if level == SOL_SOCKET && optname == SO_RCVBUF {
            return Ok(self.socket_meta(handle)?
                          .rcv_buf_size
                          .to_ne_bytes()
                          .to_vec());
        }
        if level == SOL_SOCKET && optname == SO_KEEPALIVE {
            if self.socket_meta(handle)?
                   .kind !=
               SocketKind::Tcp
            {
                return Err(NetworkError::WrongSocketType);
            }
            let enabled = self.sockets
                              .get::<tcp::Socket>(handle)
                              .keep_alive()
                              .is_some();
            return Ok((enabled as i32).to_ne_bytes()
                                      .to_vec());
        }
        if level == SOL_SOCKET &&
           matches!(optname,
                    SO_RCVTIMEO_OLD | SO_RCVTIMEO_NEW)
        {
            return Ok(millis_to_timeval(self.socket_meta(handle)?
                                            .recv_timeout_ms));
        }
        if level == SOL_SOCKET &&
           matches!(optname,
                    SO_SNDTIMEO_OLD | SO_SNDTIMEO_NEW)
        {
            return Ok(millis_to_timeval(None));
        }
        if level == IPPROTO_TCP && optname == TCP_NODELAY {
            return Ok((self.socket_meta(handle)?
                           .tcp_nodelay as i32)
                                               .to_ne_bytes()
                                               .to_vec());
        }
        if level == IPPROTO_TCP && optname == TCP_MAXSEG {
            return Ok((TCP_MSS as i32).to_ne_bytes()
                                      .to_vec());
        }
        if level == IPPROTO_TCP && optname == TCP_INFO {
            if self.socket_meta(handle)?
                   .kind !=
               SocketKind::Tcp
            {
                return Err(NetworkError::WrongSocketType);
            }
            return Ok(self.tcp_info(handle));
        }
        Err(NetworkError::Unsupported)
    }
}

/// 设置 socket 选项（支持常见 iperf 依赖的 SOL_SOCKET timeout/buffer 选项）。
pub fn socket_setsockopt(handle : SocketHandle,
                         level : usize,
                         optname : usize,
                         optval : &[u8])
                         -> Result<(), NetworkError> {
    let should_poll = with_stack_mut(NetworkError::StackUnavailable,
                                     |stack| stack.set_sockopt(handle, level, optname, optval))?;
    if should_poll {
        poll_socket_events();
    }
    Ok(())
}

/// 查询 SO_RCVTIMEO，供 syscall 阻塞接收路径换算等待 tick。
pub fn socket_recv_timeout_ms(handle : SocketHandle) -> Result<Option<u64>, NetworkError> {
    with_stack(NetworkError::StackUnavailable,
               |stack| stack.recv_timeout_ms(handle))
}

/// 获取 socket 选项。
pub fn socket_getsockopt(handle : SocketHandle,
                         level : usize,
                         optname : usize)
                         -> Result<Vec<u8>, NetworkError> {
    with_stack_mut(NetworkError::StackUnavailable,
                   |stack| stack.get_sockopt(handle, level, optname))
}
