//! `accept4(2)`：接受 TCP 连接并返回新 fd。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use alloc::boxed::Box;
use driver::network::socket_handles::TcpStreamHandle;
use driver::network::stack;
use vfs::api::handle::VfsIoHandle;

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

pub(crate) fn sys_accept4(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let addr_ptr = args.arg(1);
    let addrlen_ptr = args.arg(2);
    let _flags = args.arg(3);

    let handle = match socket_fd::lookup(fd) {
        Some(h) => h,
        None => return UserRet::from_error(ErrNo::ENOTSOCK),
    };

    // 检查是否有入连接
    match stack::socket_has_pending_accept(handle) {
        Ok(true) => {}
        Ok(false) => return UserRet::from_error(ErrNo::EAGAIN),
        Err(_) => return UserRet::from_error(ErrNo::ENOTSOCK),
    }

    let (established_handle, _port) = match stack::socket_accept(handle) {
        Ok(v) => v,
        Err(_) => return UserRet::from_error(ErrNo::ECONNRESET),
    };

    // 为新连接分配 fd
    let io_handle: Box<dyn VfsIoHandle> = Box::new(TcpStreamHandle {
        handle: established_handle,
    });
    let new_fd = match vfs::fd::alloc_fd(io_handle) {
        Ok(fd) => fd,
        Err(_) => return UserRet::from_error(ErrNo::ENOMEM),
    };
    socket_fd::register(new_fd, established_handle);

    // 写回客户端地址（如果有 addr 缓冲区）
    if addr_ptr != 0 && addrlen_ptr != 0 {
        let addr = SockAddrIn {
            sin_family: 2,          // AF_INET
            sin_port: 0u16.to_be(), // unknown client port from smoltcp
            sin_addr: [
                127, 0, 0, 1,
            ],
            sin_zero: [0; 8],
        };
        if let Ok(addrlen_val) = crate::user_copy::copy_from_user_struct::<u32>(addrlen_ptr) {
            let write_len = core::mem::size_of::<SockAddrIn>().min(addrlen_val as usize);
            let addr_bytes = unsafe {
                core::slice::from_raw_parts(
                    &addr as *const SockAddrIn as *const u8,
                    write_len,
                )
            };
            let _ = crate::user_copy::copy_to_user(addr_ptr, addr_bytes);
            let _ = copy_to_user_struct(addrlen_ptr, &(write_len as u32));
        }
    }

    UserRet::from_success(new_fd)
}
