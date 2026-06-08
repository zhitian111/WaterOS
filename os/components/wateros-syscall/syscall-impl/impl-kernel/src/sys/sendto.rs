//! `sendto(2)`：UDP 发送数据报到指定地址。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use driver::network::stack;

use crate::socket_fd;
use crate::user_copy::{copy_from_user, copy_from_user_struct};

#[repr(C)]
#[derive(Copy, Clone)]
struct SockAddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: [u8; 4],
    sin_zero: [u8; 8],
}

pub(crate) fn sys_sendto(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let buf_ptr = args.arg(1);
    let len = args.arg(2);
    let _flags = args.arg(3);
    let addr_ptr = args.arg(4);
    let addrlen = args.arg(5);

    if len == 0 {
        return UserRet::from_success(0);
    }
    if buf_ptr == 0 || len > 65536 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let socket = match socket_fd::lookup(fd) {
        Some(s) => s,
        None => return UserRet::from_error(ErrNo::ENOTSOCK),
    };
    let handle = socket.handle();

    let mut kbuf = alloc::vec![0u8; len];
    match copy_from_user(&mut kbuf, buf_ptr) {
        Ok(n) if n == len => {}
        _ => return UserRet::from_error(ErrNo::EFAULT),
    }

    // 解析目标地址
    let (ip, port) = if addr_ptr != 0 && addrlen >= 16 {
        match copy_from_user_struct::<SockAddrIn>(addr_ptr) {
            Ok(addr) => (
                addr.sin_addr,
                u16::from_be(addr.sin_port),
            ),
            Err(_) => return UserRet::from_error(ErrNo::EFAULT),
        }
    } else {
        // 没有目标地址 → 作为 TCP send（或已 connect 的 UDP）
        return match stack::socket_send(handle, &kbuf) {
            Ok(n) => UserRet::from_success(n),
            Err(e) => {
                log::warn!("[syscall] sendto failed: {}", e);
                UserRet::from_error(ErrNo::EIO)
            }
        };
    };

    match stack::socket_sendto(handle, &kbuf, ip, port) {
        Ok(n) => UserRet::from_success(n),
        Err(e) => {
            log::warn!("[syscall] sendto failed: {}", e);
            UserRet::from_error(ErrNo::EIO)
        }
    }
}
