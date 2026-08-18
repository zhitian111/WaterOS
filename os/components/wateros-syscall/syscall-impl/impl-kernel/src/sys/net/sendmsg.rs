//! `sendmsg(2)` / `recvmsg(2)` — 将 scatter/gather I/O 转换为内部 send/recv 调用。

//! 本模块代码由AI完成
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use network::{
    stack, SocketKind, SocketReceiveLease, SocketRecvError, SocketRecvFinish, SocketRef,
};
use wateros_base_config::task::SCHED_TIMER_PERIOD_MS;

use crate::fallible_buf::{try_kbuf, SYSCALL_IO_MAX};
use crate::socket_fd;
use crate::user_copy::{
    copy_from_user, copy_from_user_array, copy_from_user_struct, copy_to_user,
    copy_to_user_progress, copy_to_user_struct,
};

#[repr(C)]
#[derive(Copy, Clone)]
struct IoVec {
    /// 用户缓冲区地址。
    iov_base : usize,
    /// 缓冲区长度，单位为字节。
    iov_len : usize,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct MsgHdr {
    /// socket 地址缓冲区地址。
    msg_name : usize,
    /// 地址缓冲区长度。
    msg_namelen : u32,
    _pad0 : u32,
    msg_iov : usize,
    msg_iovlen : usize,
    msg_control : usize,
    msg_controllen : usize,
    msg_flags : i32,
    _pad1 : u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
// 本结构代码由AI完成
struct SockAddrIn {
    /// 地址族。
    sin_family : u16,
    /// 网络字节序端口。
    sin_port : u16,
    /// IPv4 地址。
    sin_addr : [u8; 4],
    /// ABI 填充。
    sin_zero : [u8; 8],
}

#[repr(C)]
#[derive(Copy, Clone, Default)]
struct UserTimespec {
    /// 秒数。
    sec : isize,
    /// 纳秒部分。
    nsec : isize,
}

const IOV_MAX : usize = 256;
const MMSG_MAX : usize = 1024;
const MSG_DONTWAIT : usize = 0x40;
const MSG_TRUNC : usize = 0x20;
const MSG_PEEK : usize = 0x02;
const MSG_OOB : usize = 0x01;
const MSG_ERRQUEUE : usize = 0x2000;
const MSG_WAITFORONE : usize = 0x10000;
const SOCKET_RECVMSG_WAIT_TICKS : usize = 4096;

// 本方法代码由AI完成
pub(crate) fn sys_sendmsg(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let msg_ptr = args.arg(1);
    let flags = args.arg(2);

    match sendmsg_one(fd, msg_ptr, flags) {
        Ok(n) => UserRet::from_success(n),
        Err(err) => UserRet::from_error(err),
    }
}

pub(crate) fn sys_sendmmsg(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let msgvec_ptr = args.arg(1);
    let vlen = args.arg(2);
    let flags = args.arg(3);

    if vlen == 0 || vlen > MMSG_MAX {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if msgvec_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    // 64 位 Linux ABI 中 mmsghdr 按 8 字节对齐：56 字节的 msghdr 后是
    // 4 字节 msg_len，并以 4 字节尾部填充将单个数组元素补齐到 64 字节。
    let entry_size = core::mem::size_of::<MsgHdr>() + core::mem::size_of::<usize>();
    let msg_len_offset = core::mem::size_of::<MsgHdr>();
    let mut sent = 0usize;
    for index in 0..vlen {
        let Some(entry_ptr) = index.checked_mul(entry_size)
                                         .and_then(|offset| msgvec_ptr.checked_add(offset))
        else {
            return if sent > 0 {
                UserRet::from_success(sent)
            } else {
                UserRet::from_error(ErrNo::EFAULT)
            };
        };
        match sendmsg_one(fd, entry_ptr, flags) {
            Ok(message_len) => {
                let Some(msg_len_ptr) = entry_ptr.checked_add(msg_len_offset) else {
                    return if sent > 0 {
                        UserRet::from_success(sent)
                    } else {
                        UserRet::from_error(ErrNo::EFAULT)
                    };
                };
                if copy_to_user_struct(msg_len_ptr, &(message_len as u32)).is_err() {
                    return if sent > 0 {
                        UserRet::from_success(sent)
                    } else {
                        UserRet::from_error(ErrNo::EFAULT)
                    };
                }
                sent += 1;
            }
            Err(error) => {
                return if sent > 0 {
                    UserRet::from_success(sent)
                } else {
                    UserRet::from_error(error)
                };
            }
        }
    }
    UserRet::from_success(sent)
}

/// 批量接收消息。带 timeout 时用非阻塞 `recvmsg` 加 scheduler sleep 驱动，
/// 从而不会让单次内部接收越过调用者给出的期限。
pub(crate) fn sys_recvmmsg(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let msgvec_ptr = args.arg(1);
    let vlen = args.arg(2);
    let flags = args.arg(3);
    let timeout_ptr = args.arg(4);

    if vlen == 0 || vlen > MMSG_MAX {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if msgvec_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let deadline = match recvmmsg_deadline(timeout_ptr) {
        Ok(deadline) => deadline,
        Err(error) => return UserRet::from_error(error),
    };
    let entry_size = core::mem::size_of::<MsgHdr>() + core::mem::size_of::<usize>();
    let msg_len_offset = core::mem::size_of::<MsgHdr>();
    let base_flags = flags & !MSG_WAITFORONE;
    let mut received = 0usize;

    while received < vlen {
        let entry_ptr = match received.checked_mul(entry_size)
                                      .and_then(|offset| msgvec_ptr.checked_add(offset))
        {
            Some(ptr) => ptr,
            None => return partial_or_error(received, ErrNo::EFAULT),
        };
        let force_nonblocking = deadline.is_some() ||
                                (received > 0 && flags & MSG_WAITFORONE != 0);
        let recv_flags = base_flags |
                         if force_nonblocking { MSG_DONTWAIT } else { 0 };
        match recvmsg_one_via_syscall(fd, entry_ptr, recv_flags) {
            Ok(message_len) => {
                let Some(msg_len_ptr) = entry_ptr.checked_add(msg_len_offset) else {
                    return partial_or_error(received, ErrNo::EFAULT);
                };
                if copy_to_user_struct(msg_len_ptr, &(message_len as u32)).is_err() {
                    return partial_or_error(received, ErrNo::EFAULT);
                }
                received += 1;
            }
            Err(ErrNo::EAGAIN) if received > 0 && flags & MSG_WAITFORONE != 0 => break,
            Err(ErrNo::EAGAIN) if deadline.is_some() => {
                if recvmmsg_deadline_expired(deadline) {
                    break;
                }
                if task::sleep_for_ticks(1) == task::TaskWaitResult::Interrupted {
                    return partial_or_error(received, ErrNo::EINTR);
                }
            }
            Err(ErrNo::EAGAIN) if received > 0 => break,
            Err(error) => return partial_or_error(received, error),
        }
        if recvmmsg_deadline_expired(deadline) {
            break;
        }
    }

    if let Err(error) = write_recvmmsg_remaining(timeout_ptr, deadline) {
        return partial_or_error(received, error);
    }
    UserRet::from_success(received)
}

fn recvmsg_one_via_syscall(fd : usize,
                           msg_ptr : usize,
                           flags : usize)
                           -> Result<usize, ErrNo> {
    let ret = sys_recvmsg(SyscallArgs::from_regs([fd, msg_ptr, flags, 0, 0, 0]));
    if ret.0 >= 0 {
        Ok(ret.0 as usize)
    } else {
        Err(ErrNo::from_raw(-ret.0).unwrap_or(ErrNo::EIO))
    }
}

fn partial_or_error(completed : usize, error : ErrNo) -> UserRet {
    if completed > 0 {
        UserRet::from_success(completed)
    } else {
        UserRet::from_error(error)
    }
}

fn recvmmsg_deadline(timeout_ptr : usize) -> Result<Option<u128>, ErrNo> {
    if timeout_ptr == 0 {
        return Ok(None);
    }
    let timeout = copy_from_user_struct::<UserTimespec>(timeout_ptr)?;
    if timeout.sec < 0 || timeout.nsec < 0 || timeout.nsec >= 1_000_000_000 {
        return Err(ErrNo::EINVAL);
    }
    let duration = (timeout.sec as u128).saturating_mul(1_000_000_000)
                                             .saturating_add(timeout.nsec as u128);
    let now = platform::wall_clock::monotonic_ns().map_err(|_| ErrNo::EIO)?;
    Ok(Some(now.saturating_add(duration)))
}

fn recvmmsg_deadline_expired(deadline : Option<u128>) -> bool {
    deadline.is_some_and(|deadline| {
        platform::wall_clock::monotonic_ns().map_or(true, |now| now >= deadline)
    })
}

fn write_recvmmsg_remaining(timeout_ptr : usize,
                            deadline : Option<u128>)
                            -> Result<(), ErrNo> {
    if timeout_ptr == 0 {
        return Ok(());
    }
    let Some(deadline) = deadline else { return Ok(()); };
    let now = platform::wall_clock::monotonic_ns().map_err(|_| ErrNo::EIO)?;
    let remaining = deadline.saturating_sub(now);
    copy_to_user_struct(timeout_ptr,
                        &UserTimespec { sec : (remaining / 1_000_000_000) as isize,
                                        nsec : (remaining % 1_000_000_000) as isize })
}

fn sendmsg_one(fd : usize, msg_ptr : usize, flags : usize) -> Result<usize, ErrNo> {

    if msg_ptr == 0 {
        return Err(ErrNo::EFAULT);
    }

    let msg : MsgHdr = match copy_from_user_struct(msg_ptr) {
        Ok(v) => v,
        Err(_) => return Err(ErrNo::EFAULT),
    };

    if msg.msg_iovlen == 0 || msg.msg_iovlen > IOV_MAX {
        return Err(ErrNo::EINVAL);
    }
    if msg.msg_iov == 0 {
        return Err(ErrNo::EFAULT);
    }

    // 读取 iovec 数组并收集数据
    let mut total_len : usize = 0;
    let iovs = match copy_from_user_array::<IoVec>(msg.msg_iov, msg.msg_iovlen) {
        Ok(iovs) => iovs,
        Err(error) => return Err(error),
    };
    for iov in &iovs {
        total_len = match total_len.checked_add(iov.iov_len) {
            Some(total) => total,
            None => return Err(ErrNo::EINVAL),
        };
        if total_len > SYSCALL_IO_MAX {
            return Err(ErrNo::EMSGSIZE);
        }
    }

    if total_len == 0 {
        return Ok(0);
    }

    // 将 iovec 数据拼接到单个缓冲区
    let mut kbuf = match try_kbuf(total_len, SYSCALL_IO_MAX) {
        Ok(buf) => buf,
        Err(err) => return Err(err),
    };
    let mut offset = 0;
    for iov in &iovs {
        if iov.iov_len > 0 {
            let dst = &mut kbuf[offset..offset + iov.iov_len];
            if copy_from_user(dst, iov.iov_base).is_err() {
                return Err(ErrNo::EFAULT);
            }
            offset += iov.iov_len;
        }
    }

    let socket = match socket_fd::lookup(fd) {
        Some(s) => s,
        None => return Err(ErrNo::ENOTSOCK),
    };
    // 有目标地址 → sendto；否则使用 connect() 保存的默认 peer。
    let destination = if msg.msg_name != 0 && msg.msg_namelen >= 16 {
        let addr : SockAddrIn = match copy_from_user_struct(msg.msg_name) {
            Ok(a) => a,
            Err(_) => return Err(ErrNo::EFAULT),
        };
        let port = u16::from_be(addr.sin_port);
        Some((addr.sin_addr, port))
    } else {
        None
    };

    let sent = match socket.kind() {
        Ok(SocketKind::Udp) => {
            super::sendto::send_udp_blocking(&socket, &kbuf, destination, flags)
        }
        Ok(SocketKind::Tcp) => socket.send(&kbuf)
                                     .map_err(super::sendto::socket_send_error_to_errno),
        Err(_) => Err(ErrNo::ENOTSOCK),
    };
    sent
}

// 本方法代码由AI完成
pub(crate) fn sys_recvmsg(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let msg_ptr = args.arg(1);
    let flags = args.arg(2);

    if msg_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if flags & MSG_OOB != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if flags & MSG_ERRQUEUE != 0 {
        return UserRet::from_error(ErrNo::EAGAIN);
    }

    let mut msg : MsgHdr = match copy_from_user_struct(msg_ptr) {
        Ok(v) => v,
        Err(_) => return UserRet::from_error(ErrNo::EFAULT),
    };

    if msg.msg_iovlen == 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if msg.msg_iovlen > IOV_MAX {
        return UserRet::from_error(ErrNo::EMSGSIZE);
    }
    if msg.msg_iov == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    // 读取 iovec 数组并计算总接收空间
    let mut total_len : usize = 0;
    let iovs = match copy_from_user_array::<IoVec>(msg.msg_iov, msg.msg_iovlen) {
        Ok(iovs) => iovs,
        Err(error) => return UserRet::from_error(error),
    };
    for iov in &iovs {
        total_len = match total_len.checked_add(iov.iov_len) {
            Some(total) if total <= SYSCALL_IO_MAX => total,
            _ => return UserRet::from_error(ErrNo::EINVAL),
        };
    }

    if total_len == 0 {
        return UserRet::from_success(0);
    }

    let socket = match socket_fd::lookup_or_errno(fd) {
        Ok(socket) => socket,
        Err(error) => return UserRet::from_error(error),
    };
    let kind = match socket.kind() {
        Ok(kind) => kind,
        Err(_) => return UserRet::from_error(ErrNo::ENOTSOCK),
    };
    let lease = match recvmsg_receive_blocking(&socket, flags, total_len) {
        Ok(Some(lease)) => lease,
        Ok(None) => return UserRet::from_success(0),
        Err(error) => return UserRet::from_error(error),
    };
    let (from_ip, from_port) = lease.source();
    let staged_len = lease.bytes().len();
    let datagram_len = lease.datagram_len();

    // 将数据分发到 iovec 缓冲区
    let mut offset = 0;
    let mut remaining = staged_len;
    let mut copy_error = None;
    for iov in &iovs {
        if remaining == 0 {
            break;
        }
        let chunk = iov.iov_len
                       .min(remaining);
        if chunk > 0 {
            let progress = copy_to_user_progress(iov.iov_base,
                                                 &lease.bytes()[offset..offset + chunk]);
            offset += progress.copied;
            remaining -= progress.copied;
            if let Some(error) = progress.error {
                copy_error = Some(error);
                break;
            }
        }
    }

    if copy_error.is_some() {
        if flags & MSG_PEEK != 0 {
            let _ = lease.finish(0, false);
            return if offset > 0 {
                UserRet::from_success(offset)
            } else {
                UserRet::from_error(ErrNo::EFAULT)
            };
        }
        return match lease.finish(offset, false) {
            Ok(SocketRecvFinish::Bytes(copied)) => UserRet::from_success(copied),
            Ok(SocketRecvFinish::Fault) => UserRet::from_error(ErrNo::EFAULT),
            Err(error) => UserRet::from_error(recv_error_to_errno(error)),
        };
    }

    // 写回发送方地址到 msg_name
    if kind == SocketKind::Udp && msg.msg_name != 0 {
        let addr = SockAddrIn { sin_family : 2, // AF_INET
                                sin_port : from_port.to_be(),
                                sin_addr : from_ip,
                                sin_zero : [0; 8] };
        let write_len = core::mem::size_of::<SockAddrIn>().min(msg.msg_namelen as usize);
        let addr_bytes = unsafe {
            core::slice::from_raw_parts(&addr as *const SockAddrIn as *const u8,
                                        write_len)
        };
        if copy_to_user(msg.msg_name, addr_bytes).is_err() {
            let _ = lease.finish(0, false);
            return UserRet::from_error(ErrNo::EFAULT);
        }
    }
    msg.msg_flags = if kind == SocketKind::Udp && datagram_len > staged_len {
        MSG_TRUNC as i32
    } else {
        0
    };
    if copy_to_user_struct(msg_ptr, &msg).is_err() {
        let _ = lease.finish(0, false);
        return UserRet::from_error(ErrNo::EFAULT);
    }

    if flags & MSG_PEEK != 0 {
        let _ = lease.finish(0, false);
        return if kind == SocketKind::Udp && flags & MSG_TRUNC != 0 {
            UserRet::from_success(datagram_len)
        } else {
            UserRet::from_success(staged_len)
        };
    }

    match lease.finish(staged_len, true) {
        Ok(SocketRecvFinish::Bytes(copied)) => {
            if kind == SocketKind::Udp && flags & MSG_TRUNC != 0 {
                UserRet::from_success(datagram_len)
            } else {
                UserRet::from_success(copied)
            }
        }
        Ok(SocketRecvFinish::Fault) => UserRet::from_error(ErrNo::EFAULT),
        Err(error) => UserRet::from_error(recv_error_to_errno(error)),
    }
}

fn recvmsg_receive_blocking(socket : &SocketRef,
                            flags : usize,
                            max_len : usize)
                            -> Result<Option<SocketReceiveLease>, ErrNo> {
    let nonblocking = socket_fd::is_nonblocking_socket(socket) || (flags & MSG_DONTWAIT) != 0;
    let wait_ticks = socket_recv_wait_ticks(socket, SOCKET_RECVMSG_WAIT_TICKS);
    for _ in 0..wait_ticks {
        drive_network_stack();
        match socket.prepare_receive(max_len) {
            Ok(lease) => return Ok(Some(lease)),
            Err(SocketRecvError::Finished) => return Ok(None),
            Err(SocketRecvError::Busy | SocketRecvError::Empty) => {}
            Err(error) => return Err(recv_error_to_errno(error)),
        }
        if nonblocking {
            return Err(ErrNo::EAGAIN);
        }
        task::sleep_for_ticks(1);
    }
    Err(ErrNo::EAGAIN)
}

fn recv_error_to_errno(error : SocketRecvError) -> ErrNo {
    match error {
        SocketRecvError::Busy | SocketRecvError::Empty => ErrNo::EAGAIN,
        SocketRecvError::Finished => ErrNo::EIO,
        SocketRecvError::InvalidSocket => ErrNo::ENOTSOCK,
        SocketRecvError::NoMemory => ErrNo::ENOMEM,
        SocketRecvError::Io => ErrNo::EIO,
    }
}

fn socket_recv_wait_ticks(socket : &SocketRef, default_ticks : usize) -> usize {
    match socket.recv_timeout_ms() {
        Ok(Some(ms)) => {
            let tick_ms = (SCHED_TIMER_PERIOD_MS as u64).max(1);
            let ticks = ms.saturating_add(tick_ms - 1) / tick_ms;
            usize::try_from(ticks).unwrap_or(usize::MAX)
                                  .max(1)
        }
        _ => default_ticks,
    }
}

fn drive_network_stack() {
    match platform::timer::now_duration() {
        Ok(now) => {
            let millis = now.as_millis()
                            .min(i64::MAX as u128) as i64;
            stack::poll_at_millis(millis);
        }
        Err(_) => stack::poll(),
    }
    stack::poll_socket_events();
}
