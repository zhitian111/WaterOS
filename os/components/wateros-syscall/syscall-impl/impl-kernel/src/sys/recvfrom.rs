//! `recvfrom(2)`：接收 TCP/UDP 数据。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use driver::network::stack;

use crate::socket_fd;
use crate::user_copy::{copy_to_user, copy_to_user_struct};

#[repr(C)]
#[derive(Copy, Clone)]
struct SockAddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: [u8; 4],
    sin_zero: [u8; 8],
}

pub(crate) fn sys_recvfrom(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let buf_ptr = args.arg(1);
    let len = args.arg(2);
    let _flags = args.arg(3);
    let addr_ptr = args.arg(4);
    let addrlen_ptr = args.arg(5);

    if len == 0 {
        return UserRet::from_success(0);
    }
    if buf_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let handle = match socket_fd::lookup(fd) {
        Some(h) => h,
        None => return UserRet::from_error(ErrNo::ENOTSOCK),
    };

    // 根据 socket 类型选择 recv 或 recvfrom
    match stack::socket_kind(handle) {
        Ok(driver::network::stack::SocketKind::Tcp) => {
            let mut kbuf = alloc::vec![0u8; len];
            match stack::socket_recv(handle, &mut kbuf) {
                Ok(n) if n > 0 => {
                    let _ = copy_to_user(buf_ptr, &kbuf[..n]);
                    UserRet::from_success(n)
                }
                _ => UserRet::from_error(ErrNo::EAGAIN),
            }
        }
        Ok(driver::network::stack::SocketKind::Udp) => {
            let mut kbuf = alloc::vec![0u8; len];
            match stack::socket_recvfrom(handle, &mut kbuf) {
                Ok((n, ip, port)) if n > 0 => {
                    if copy_to_user(buf_ptr, &kbuf[..n]).is_err() {
                        return UserRet::from_error(ErrNo::EFAULT);
                    }
                    if addr_ptr != 0 && addrlen_ptr != 0 {
                        if let Ok(addrlen_val) = crate::user_copy::copy_from_user_struct::<u32>(addrlen_ptr) {
                            let sockaddr = SockAddrIn {
                                sin_family: 2,
                                sin_port: port.to_be(),
                                sin_addr: ip,
                                sin_zero: [0; 8],
                            };
                            let write_len: usize = core::mem::size_of::<SockAddrIn>().min(addrlen_val as usize);
                            let addr_bytes = unsafe {
                                core::slice::from_raw_parts(&sockaddr as *const SockAddrIn as *const u8, write_len)
                            };
                            let _ = copy_to_user(addr_ptr, addr_bytes);
                            let _ = copy_to_user_struct(addrlen_ptr, &(write_len as u32));
                        }
                    }
                    UserRet::from_success(n)
                }
                _ => UserRet::from_error(ErrNo::EAGAIN),
            }
        }
        _ => UserRet::from_error(ErrNo::ENOTSOCK),
    }
}
