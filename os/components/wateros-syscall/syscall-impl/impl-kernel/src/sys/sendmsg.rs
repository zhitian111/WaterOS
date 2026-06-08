//! `sendmsg(2)` / `recvmsg(2)` — 将 scatter/gather I/O 转换为内部 send/recv 调用。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use driver::network::stack;

use crate::socket_fd;
use crate::user_copy::{copy_from_user, copy_from_user_struct, copy_to_user};

#[repr(C)]
#[derive(Copy, Clone)]
struct IoVec {
    iov_base: usize,
    iov_len: usize,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct MsgHdr {
    msg_name: usize,
    msg_namelen: u32,
    _pad0: u32,
    msg_iov: usize,
    msg_iovlen: usize,
    msg_control: usize,
    msg_controllen: usize,
    msg_flags: i32,
    _pad1: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct SockAddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: [u8; 4],
    sin_zero: [u8; 8],
}

const IOV_MAX: usize = 256;

pub(crate) fn sys_sendmsg(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let msg_ptr = args.arg(1);
    let _flags = args.arg(2);

    if msg_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let msg: MsgHdr = match copy_from_user_struct(msg_ptr) {
        Ok(v) => v,
        Err(_) => return UserRet::from_error(ErrNo::EFAULT),
    };

    if msg.msg_iovlen == 0 || msg.msg_iovlen > IOV_MAX {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if msg.msg_iov == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    // 读取 iovec 数组并收集数据
    let iov_size = core::mem::size_of::<IoVec>();
    let mut total_len: usize = 0;
    let mut iovs: alloc::vec::Vec<IoVec> = alloc::vec![];

    for i in 0..msg.msg_iovlen {
        let iov: IoVec = match copy_from_user_struct(msg.msg_iov + i * iov_size) {
            Ok(v) => v,
            Err(_) => return UserRet::from_error(ErrNo::EFAULT),
        };
        total_len = total_len.saturating_add(iov.iov_len);
        if total_len > 65536 {
            return UserRet::from_error(ErrNo::EMSGSIZE);
        }
        iovs.push(iov);
    }

    if total_len == 0 {
        return UserRet::from_success(0);
    }

    // 将 iovec 数据拼接到单个缓冲区
    let mut kbuf = alloc::vec![0u8; total_len];
    let mut offset = 0;
    for iov in &iovs {
        if iov.iov_len > 0 {
            let dst = &mut kbuf[offset..offset + iov.iov_len];
            if copy_from_user(dst, iov.iov_base).is_err() {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            offset += iov.iov_len;
        }
    }

    let socket = match socket_fd::lookup(fd) {
        Some(s) => s,
        None => return UserRet::from_error(ErrNo::ENOTSOCK),
    };
    let handle = socket.handle();

    // 有目标地址 → sendto；否则 → send
    let sent = if msg.msg_name != 0 && msg.msg_namelen >= 16 {
        let addr: SockAddrIn = match copy_from_user_struct(msg.msg_name) {
            Ok(a) => a,
            Err(_) => return UserRet::from_error(ErrNo::EFAULT),
        };
        let port = u16::from_be(addr.sin_port);
        stack::socket_sendto(handle, &kbuf, addr.sin_addr, port)
    } else {
        stack::socket_send(handle, &kbuf)
    };

    match sent {
        Ok(n) => UserRet::from_success(n),
        Err(e) => {
            log::warn!("[syscall] sendmsg failed: {}", e);
            UserRet::from_error(ErrNo::EIO)
        }
    }
}

pub(crate) fn sys_recvmsg(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let msg_ptr = args.arg(1);
    let _flags = args.arg(2);

    if msg_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let msg: MsgHdr = match copy_from_user_struct(msg_ptr) {
        Ok(v) => v,
        Err(_) => return UserRet::from_error(ErrNo::EFAULT),
    };

    if msg.msg_iovlen == 0 || msg.msg_iovlen > IOV_MAX {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if msg.msg_iov == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    // 读取 iovec 数组并计算总接收空间
    let iov_size = core::mem::size_of::<IoVec>();
    let mut total_len: usize = 0;
    let mut iovs: alloc::vec::Vec<IoVec> = alloc::vec![];

    for i in 0..msg.msg_iovlen {
        let iov: IoVec = match copy_from_user_struct(msg.msg_iov + i * iov_size) {
            Ok(v) => v,
            Err(_) => return UserRet::from_error(ErrNo::EFAULT),
        };
        total_len = total_len.saturating_add(iov.iov_len);
        iovs.push(iov);
    }

    if total_len == 0 {
        return UserRet::from_success(0);
    }

    let socket = match socket_fd::lookup(fd) {
        Some(s) => s,
        None => return UserRet::from_error(ErrNo::ENOTSOCK),
    };
    let handle = socket.handle();

    let mut kbuf = alloc::vec![0u8; total_len];

    // 根据 socket 类型接收，写回发送方地址
    let (n, from_ip, from_port) = match stack::socket_kind(handle) {
        Ok(stack::SocketKind::Tcp) => match stack::socket_recv(handle, &mut kbuf) {
            Ok(n) if n > 0 => (n, [0u8; 4], 0u16),
            _ => return UserRet::from_error(ErrNo::EAGAIN),
        },
        Ok(stack::SocketKind::Udp) => match stack::socket_recvfrom(handle, &mut kbuf) {
            Ok((n, ip, port)) if n > 0 => (n, ip, port),
            _ => return UserRet::from_error(ErrNo::EAGAIN),
        },
        _ => return UserRet::from_error(ErrNo::ENOTSOCK),
    };

    // 将数据分发到 iovec 缓冲区
    let mut offset = 0;
    let mut remaining = n;
    for iov in &iovs {
        if remaining == 0 {
            break;
        }
        let chunk = iov
            .iov_len
            .min(remaining);
        if chunk > 0 {
            if copy_to_user(
                iov.iov_base,
                &kbuf[offset..offset + chunk],
            )
            .is_err()
            {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            offset += chunk;
            remaining -= chunk;
        }
    }

    // 写回发送方地址到 msg_name
    if msg.msg_name != 0 && (from_ip != [0; 4] || from_port != 0) {
        let addr = SockAddrIn {
            sin_family: 2, // AF_INET
            sin_port: from_port.to_be(),
            sin_addr: from_ip,
            sin_zero: [0; 8],
        };
        let write_len = core::mem::size_of::<SockAddrIn>().min(msg.msg_namelen as usize);
        let addr_bytes = unsafe {
            core::slice::from_raw_parts(
                &addr as *const SockAddrIn as *const u8,
                write_len,
            )
        };
        let _ = copy_to_user(msg.msg_name, addr_bytes);
    }

    UserRet::from_success(n)
}
