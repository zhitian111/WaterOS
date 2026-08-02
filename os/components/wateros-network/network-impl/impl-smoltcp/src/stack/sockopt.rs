//! Linux socket 选项与 smoltcp 状态之间的转换。

use alloc::vec;
use alloc::vec::Vec;
use smoltcp::iface::SocketHandle;
use smoltcp::socket::tcp;
use smoltcp::time::Duration;

use super::poll::poll_socket_events;
use super::socket::socket_send_capacity;
use super::state::{SocketMeta, NETWORK_STACK, TCP_BUFFER_SIZE, TCP_MSS};
use super::tcp::socket_is_connected;
use super::types::{NetworkError, SocketKind};

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
             .any(|&b| b != 0))
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


/// 设置 socket 选项（支持常见 iperf 依赖的 SOL_SOCKET timeout/buffer 选项）。
pub fn socket_setsockopt(handle : SocketHandle,
                         level : usize,
                         optname : usize,
                         optval : &[u8])
                         -> Result<(), NetworkError> {
    const SOL_SOCKET : usize = 1;
    const SOL_IP : usize = 0;
    const IPPROTO_IP : usize = 0;
    const SO_REUSEADDR : usize = 2;
    const SO_DONTROUTE : usize = 5;
    const SO_REUSEPORT : usize = 15;
    const SO_SNDBUF : usize = 7;
    const SO_RCVBUF : usize = 8;
    const SO_RCVTIMEO_OLD : usize = 20;
    const SO_SNDTIMEO_OLD : usize = 21;
    const SO_RCVTIMEO_NEW : usize = 66;
    const SO_SNDTIMEO_NEW : usize = 67;
    const IPPROTO_TCP : usize = 6;
    const TCP_NODELAY : usize = 1;
    const IP_ADD_MEMBERSHIP : usize = 35;
    const IP_DROP_MEMBERSHIP : usize = 36;
    const MCAST_JOIN_GROUP : usize = 42;
    const MCAST_LEAVE_GROUP : usize = 45;

    if (level == SOL_IP || level == IPPROTO_IP) &&
       matches!(optname,
                IP_ADD_MEMBERSHIP | MCAST_JOIN_GROUP)
    {
        let group = parse_ipv4_mcast_group(optval)?;
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut()
                         .ok_or(NetworkError::StackUnavailable)?;
        let meta = stack.metas
                        .get_mut(&handle)
                        .ok_or(NetworkError::InvalidSocket)?;
        mcast_join(meta, group);
        return Ok(());
    }
    if (level == SOL_IP || level == IPPROTO_IP) &&
       matches!(optname,
                IP_DROP_MEMBERSHIP | MCAST_LEAVE_GROUP)
    {
        let group = parse_ipv4_mcast_group(optval)?;
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut()
                         .ok_or(NetworkError::StackUnavailable)?;
        let meta = stack.metas
                        .get_mut(&handle)
                        .ok_or(NetworkError::InvalidSocket)?;
        return mcast_leave(meta, group);
    }

    if level == SOL_SOCKET && matches!(optname, SO_REUSEADDR | SO_REUSEPORT) {
        return Ok(());
    }
    // 对回环目标没有网关可绕行，SO_DONTROUTE 的开启与关闭不会改变
    // 当前数据路径；仍解析布尔参数，避免把畸形 optval 当作成功。
    if level == SOL_SOCKET && optname == SO_DONTROUTE {
        let _enabled = sockopt_bool(optval)?;
        return Ok(());
    }
    // netperf/iperf 会 setsockopt(SO_SNDBUF/SO_RCVBUF)；记录请求值供 getsockopt 回报。
    if level == SOL_SOCKET && optname == SO_SNDBUF {
        let value = sockopt_i32(optval)?;
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut()
                         .ok_or(NetworkError::StackUnavailable)?;
        let meta = stack.metas
                        .get_mut(&handle)
                        .ok_or(NetworkError::InvalidSocket)?;
        meta.snd_buf_size = value.max(0);
        return Ok(());
    }
    if level == SOL_SOCKET && optname == SO_RCVBUF {
        let value = sockopt_i32(optval)?;
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut()
                         .ok_or(NetworkError::StackUnavailable)?;
        let meta = stack.metas
                        .get_mut(&handle)
                        .ok_or(NetworkError::InvalidSocket)?;
        meta.rcv_buf_size = value.max(0);
        return Ok(());
    }
    if level == SOL_SOCKET && (optname == SO_RCVTIMEO_OLD || optname == SO_RCVTIMEO_NEW) {
        let timeout_ms = timeval_to_millis(optval)?;
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut()
                         .ok_or(NetworkError::StackUnavailable)?;
        let meta = stack.metas
                        .get_mut(&handle)
                        .ok_or(NetworkError::InvalidSocket)?;
        meta.recv_timeout_ms = timeout_ms;
        return Ok(());
    }
    if level == SOL_SOCKET && (optname == SO_SNDTIMEO_OLD || optname == SO_SNDTIMEO_NEW) {
        let _ = timeval_to_millis(optval)?;
        return Ok(());
    }
    if level == IPPROTO_TCP && optname == TCP_NODELAY {
        let enabled = sockopt_bool(optval)?;
        let mut guard = NETWORK_STACK.lock();
        let stack = guard.as_mut()
                         .ok_or(NetworkError::StackUnavailable)?;
        let kind = stack.metas
                        .get(&handle)
                        .map(|meta| meta.kind)
                        .ok_or(NetworkError::InvalidSocket)?;
        if kind != SocketKind::Tcp {
            return Err(NetworkError::WrongSocketType);
        }
        let socket = stack.sockets
                          .get_mut::<tcp::Socket>(handle);
        socket.set_nagle_enabled(!enabled);
        socket.set_ack_delay(if enabled {
                                 None
                             } else {
                                 Some(Duration::from_millis(10))
                             });
        let meta = stack.metas
                        .get_mut(&handle)
                        .ok_or(NetworkError::InvalidSocket)?;
        meta.tcp_nodelay = enabled;
        drop(guard);
        poll_socket_events();
        return Ok(());
    }
    Err(NetworkError::Unsupported)
}


/// 查询 SO_RCVTIMEO，供 syscall 阻塞接收路径换算等待 tick。
pub fn socket_recv_timeout_ms(handle : SocketHandle) -> Result<Option<u64>, NetworkError> {
    let guard = NETWORK_STACK.lock();
    let stack = guard.as_ref()
                     .ok_or(NetworkError::StackUnavailable)?;
    let meta = stack.metas
                    .get(&handle)
                    .ok_or(NetworkError::InvalidSocket)?;
    Ok(meta.recv_timeout_ms)
}


fn write_u32(buf : &mut [u8], offset : usize, value : u32) {
    if offset + 4 <= buf.len() {
        buf[offset..offset + 4].copy_from_slice(&value.to_ne_bytes());
    }
}


fn tcp_info(handle : SocketHandle) -> Vec<u8> {
    const TCP_INFO_LEN : usize = 256;
    const TCP_ESTABLISHED : u8 = 1;
    const TCP_CLOSE : u8 = 7;

    let mut out = vec![0u8; TCP_INFO_LEN];
    let connected = socket_is_connected(handle).unwrap_or(false);
    out[0] = if connected {
        TCP_ESTABLISHED
    } else {
        TCP_CLOSE
    };

    let rcv_space = {
        let guard = NETWORK_STACK.lock();
        guard.as_ref()
             .and_then(|stack| {
                 stack.metas
                      .get(&handle)
             })
             .map(|meta| {
                 meta.rcv_buf_size
                     .max(0) as u32
             })
             .unwrap_or(TCP_BUFFER_SIZE as u32)
    };

    let send_capacity = socket_send_capacity(handle).unwrap_or(0) as u32;
    let cwnd_segments = (send_capacity / TCP_MSS).clamp(2, 64);

    // Linux uapi struct tcp_info offsets used by iperf3.
    write_u32(&mut out, 8, 200_000); // tcpi_rto, usec
    write_u32(&mut out, 16, TCP_MSS); // tcpi_snd_mss
    write_u32(&mut out, 20, TCP_MSS); // tcpi_rcv_mss
    write_u32(&mut out, 60, 1500); // tcpi_pmtu
    write_u32(&mut out, 64, TCP_BUFFER_SIZE as u32); // tcpi_rcv_ssthresh
    write_u32(&mut out, 68, 1_000); // tcpi_rtt, usec
    write_u32(&mut out, 72, 250); // tcpi_rttvar, usec
    write_u32(&mut out, 76, u32::MAX); // tcpi_snd_ssthresh
    write_u32(&mut out, 80, cwnd_segments); // tcpi_snd_cwnd, packets
    write_u32(&mut out, 84, TCP_MSS); // tcpi_advmss
    write_u32(&mut out, 88, 3); // tcpi_reordering
    write_u32(&mut out, 96, rcv_space); // tcpi_rcv_space
    write_u32(&mut out, 100, 0); // tcpi_total_retrans
    write_u32(&mut out, 228, rcv_space); // tcpi_snd_wnd on newer Linux
    out
}


/// 获取 socket 选项（极简 stub）。
pub fn socket_getsockopt(handle : SocketHandle,
                         level : usize,
                         optname : usize)
                         -> Result<Vec<u8>, NetworkError> {
    const SOL_SOCKET : usize = 1;
    const SO_ERROR : usize = 4;
    const SO_SNDBUF : usize = 7;
    const SO_RCVBUF : usize = 8;
    const SO_RCVTIMEO_OLD : usize = 20;
    const SO_SNDTIMEO_OLD : usize = 21;
    const SO_RCVTIMEO_NEW : usize = 66;
    const SO_SNDTIMEO_NEW : usize = 67;
    const IPPROTO_TCP : usize = 6;
    const TCP_NODELAY : usize = 1;
    const TCP_MAXSEG : usize = 2;
    const TCP_INFO : usize = 11;

    if level == SOL_SOCKET && optname == SO_ERROR {
        return Ok(0i32.to_ne_bytes()
                      .to_vec());
    }
    if level == SOL_SOCKET && optname == SO_SNDBUF {
        let value = {
            let guard = NETWORK_STACK.lock();
            let stack = guard.as_ref()
                             .ok_or(NetworkError::StackUnavailable)?;
            let meta = stack.metas
                            .get(&handle)
                            .ok_or(NetworkError::InvalidSocket)?;
            meta.snd_buf_size
        };
        return Ok(value.to_ne_bytes()
                       .to_vec());
    }
    if level == SOL_SOCKET && optname == SO_RCVBUF {
        let value = {
            let guard = NETWORK_STACK.lock();
            let stack = guard.as_ref()
                             .ok_or(NetworkError::StackUnavailable)?;
            let meta = stack.metas
                            .get(&handle)
                            .ok_or(NetworkError::InvalidSocket)?;
            meta.rcv_buf_size
        };
        return Ok(value.to_ne_bytes()
                       .to_vec());
    }
    if level == SOL_SOCKET && (optname == SO_RCVTIMEO_OLD || optname == SO_RCVTIMEO_NEW) {
        let timeout = {
            let guard = NETWORK_STACK.lock();
            let stack = guard.as_ref()
                             .ok_or(NetworkError::StackUnavailable)?;
            let meta = stack.metas
                            .get(&handle)
                            .ok_or(NetworkError::InvalidSocket)?;
            meta.recv_timeout_ms
        };
        return Ok(millis_to_timeval(timeout));
    }
    if level == SOL_SOCKET && (optname == SO_SNDTIMEO_OLD || optname == SO_SNDTIMEO_NEW) {
        return Ok(millis_to_timeval(None));
    }
    if level == IPPROTO_TCP && optname == TCP_NODELAY {
        let enabled = {
            let guard = NETWORK_STACK.lock();
            let stack = guard.as_ref()
                             .ok_or(NetworkError::StackUnavailable)?;
            let meta = stack.metas
                            .get(&handle)
                            .ok_or(NetworkError::InvalidSocket)?;
            meta.tcp_nodelay
        };
        return Ok((enabled as i32).to_ne_bytes()
                                  .to_vec());
    }
    if level == IPPROTO_TCP && optname == TCP_MAXSEG {
        return Ok((TCP_MSS as i32).to_ne_bytes()
                                  .to_vec());
    }
    if level == IPPROTO_TCP && optname == TCP_INFO {
        return Ok(tcp_info(handle));
    }
    Err(NetworkError::Unsupported)
}
