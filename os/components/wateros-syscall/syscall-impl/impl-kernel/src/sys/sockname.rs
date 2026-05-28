//! `getsockname(2)` / `getpeername(2)` — 获取 socket 地址。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use driver::network::stack;

use crate::socket_fd;
use crate::user_copy::copy_to_user_struct;

#[repr(C)]
#[derive(Copy, Clone)]
struct SockAddrIn {
    sin_family: u16,
    sin_port: u16,
    sin_addr: [u8; 4],
    sin_zero: [u8; 8],
}

pub(crate) fn sys_getsockname(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let addr_ptr = args.arg(1);
    let addrlen_ptr = args.arg(2);

    let handle = match socket_fd::lookup(fd) {
        Some(h) => h,
        None => return UserRet::from_error(ErrNo::ENOTSOCK),
    };

    let port: u16 = match stack::socket_local_port(handle) {
        Ok(p) => p,
        Err(_) => 0,
    };

    let addr = SockAddrIn {
        sin_family: 2, // AF_INET
        sin_port: port.to_be(),
        sin_addr: [127, 0, 0, 1],
        sin_zero: [0; 8],
    };

    if addr_ptr != 0 && addrlen_ptr != 0 {
        if let Ok(addrlen_val) = crate::user_copy::copy_from_user_struct::<u32>(addrlen_ptr) {
            let write_len: usize = core::mem::size_of::<SockAddrIn>().min(addrlen_val as usize);
            let addr_bytes = unsafe {
                core::slice::from_raw_parts(&addr as *const SockAddrIn as *const u8, write_len)
            };
            let _ = crate::user_copy::copy_to_user(addr_ptr, addr_bytes);
            let _ = copy_to_user_struct(addrlen_ptr, &(write_len as u32));
        }
    }

    UserRet::from_success(0)
}

pub(crate) fn sys_getpeername(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let addr_ptr = args.arg(1);
    let addrlen_ptr = args.arg(2);

    let handle = match socket_fd::lookup(fd) {
        Some(h) => h,
        None => return UserRet::from_error(ErrNo::ENOTSOCK),
    };

    let (ip, port) = match stack::socket_peername(handle) {
        Ok(v) => v,
        Err(_) => return UserRet::from_error(ErrNo::ENOTCONN),
    };

    let addr = SockAddrIn {
        sin_family: 2, // AF_INET
        sin_port: port.to_be(),
        sin_addr: ip,
        sin_zero: [0; 8],
    };

    if addr_ptr != 0 && addrlen_ptr != 0 {
        if let Ok(addrlen_val) = crate::user_copy::copy_from_user_struct::<u32>(addrlen_ptr) {
            let write_len: usize = core::mem::size_of::<SockAddrIn>().min(addrlen_val as usize);
            let addr_bytes = unsafe {
                core::slice::from_raw_parts(&addr as *const SockAddrIn as *const u8, write_len)
            };
            let _ = crate::user_copy::copy_to_user(addr_ptr, addr_bytes);
            let _ = copy_to_user_struct(addrlen_ptr, &(write_len as u32));
        }
    }

    UserRet::from_success(0)
}
