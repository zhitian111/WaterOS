//! `write(2)`：fd 1/2 走控制台；pipe 写端走 IPC。

extern crate alloc;

use alloc::vec::Vec;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::user_copy::{copy_from_user, copy_from_user_struct};
use crate::vfs_util::vfs_error_to_errno;

#[repr(C)]
#[derive(Clone, Copy)]
struct UserIoVec {
    base : usize,
    len : usize,
}

pub(crate) fn sys_write(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let ptr = args.arg(1);
    let len = args.arg(2);
    if len == 0 {
        return UserRet::from_success(0);
    }
    if ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if len > 4 * 1024 * 1024 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let mut kbuf = Vec::with_capacity(len);
    kbuf.resize(len, 0);
    match copy_from_user(&mut kbuf, ptr) {
        Ok(n) if n == len => {}
        _ => return UserRet::from_error(ErrNo::EFAULT),
    }
    match vfs::fd::with_current_io(fd, |handle| handle.write(&kbuf)) {
        Ok(n) => UserRet::from_success(n),
        Err(err) => UserRet::from_error(vfs_error_to_errno(err)),
    }
}

pub(crate) fn sys_writev(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let iov_ptr = args.arg(1);
    let iovcnt = args.arg(2);
    if iovcnt == 0 {
        return UserRet::from_success(0);
    }
    if iov_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if iovcnt > 1024 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let iov_size = core::mem::size_of::<UserIoVec>();
    let mut out = Vec::new();
    for i in 0..iovcnt {
        let iov = match copy_from_user_struct::<UserIoVec>(iov_ptr + i * iov_size) {
            Ok(v) => v,
            Err(e) => return UserRet::from_error(e),
        };
        if iov.len == 0 {
            continue;
        }
        if iov.base == 0 {
            return UserRet::from_error(ErrNo::EFAULT);
        }
        let new_len = match out.len().checked_add(iov.len) {
            Some(v) => v,
            None => return UserRet::from_error(ErrNo::EINVAL),
        };
        if new_len > 4 * 1024 * 1024 {
            return UserRet::from_error(ErrNo::EINVAL);
        }
        let old_len = out.len();
        out.resize(new_len, 0);
        match copy_from_user(&mut out[old_len..], iov.base) {
            Ok(n) if n == iov.len => {}
            _ => return UserRet::from_error(ErrNo::EFAULT),
        }
    }

    match vfs::fd::with_current_io(fd, |handle| handle.write(&out)) {
        Ok(n) => UserRet::from_success(n),
        Err(err) => UserRet::from_error(vfs_error_to_errno(err)),
    }
}
