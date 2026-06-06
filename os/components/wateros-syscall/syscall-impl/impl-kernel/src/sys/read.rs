//! `read(2)`：支持 pipe 读端；stdin 暂未接真实输入。

extern crate alloc;

use alloc::vec::Vec;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::user_copy::{copy_from_user_struct, copy_to_user};
use crate::vfs_util::vfs_error_to_errno;

#[repr(C)]
#[derive(Clone, Copy)]
struct UserIoVec {
    base: usize,
    len: usize,
}

pub(crate) fn sys_read(args : SyscallArgs) -> UserRet {
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
    let n = match vfs::fd::with_current_io(fd, |handle| handle.read(&mut kbuf)) {
        Ok(n) => n,
        Err(err) => return UserRet::from_error(vfs_error_to_errno(err)),
    };
    if n == 0 {
        return UserRet::from_success(0);
    }
    if fd >= 10 && n <= 4096 {
        if let Ok(text) = core::str::from_utf8(&kbuf[..n]) {
            log::trace!("[read-debug] fd={fd} n={n} text={text:?}");
        }
    }
    match copy_to_user(ptr, &kbuf[..n]) {
        Ok(written) if written == n => UserRet::from_success(n),
        _ => UserRet::from_error(ErrNo::EFAULT),
    }
}

pub(crate) fn sys_readv(args : SyscallArgs) -> UserRet {
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
    let mut total = 0usize;
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
        if iov.len > 4 * 1024 * 1024 {
            return UserRet::from_error(ErrNo::EINVAL);
        }
        if total.checked_add(iov.len).is_none() {
            return UserRet::from_error(ErrNo::EINVAL);
        }

        let mut kbuf = Vec::with_capacity(iov.len);
        kbuf.resize(iov.len, 0);
        let n = match vfs::fd::with_current_io(fd, |handle| handle.read(&mut kbuf)) {
            Ok(n) => n,
            Err(err) => {
                return if total > 0 {
                    UserRet::from_success(total)
                } else {
                    UserRet::from_error(vfs_error_to_errno(err))
                };
            }
        };
        if n == 0 {
            return UserRet::from_success(total);
        }
        match copy_to_user(iov.base, &kbuf[..n]) {
            Ok(written) if written == n => {}
            _ => return UserRet::from_error(ErrNo::EFAULT),
        }
        total += n;
        if n < iov.len {
            return UserRet::from_success(total);
        }
    }

    UserRet::from_success(total)
}
