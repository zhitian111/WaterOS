//! `mincore(2)`：查询用户 VMA 中每一页当前是否已有物理映射。

use api_v0::{ErrNo, SyscallArgs, UserRet};
use mm::api::{addr::PAGE_SIZE, error::MmError};

use crate::fallible_buf::{try_kbuf, SYSCALL_IO_MAX};
use crate::mm_util::{mm_err_to_errno, require_user_aspace};
use crate::user_copy::copy_to_user;

pub(crate) fn sys_mincore(args : SyscallArgs) -> UserRet {
    let addr = args.arg(0);
    let len = args.arg(1);
    let vector = args.arg(2);
    if addr & (PAGE_SIZE - 1) != 0 || len == 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if addr.checked_add(len).is_none() || vector == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let pages = match len.checked_add(PAGE_SIZE - 1) {
        Some(rounded) => rounded / PAGE_SIZE,
        None => return UserRet::from_error(ErrNo::EINVAL),
    };
    let mut residency = match try_kbuf(pages, SYSCALL_IO_MAX) {
        Ok(buffer) => buffer,
        Err(error) => return UserRet::from_error(error),
    };
    let aspace = match require_user_aspace("mincore") {
        Ok(aspace) => aspace,
        Err(error) => return UserRet::from_error(error),
    };
    if let Err(error) = mm::kernel_mm::mincore_user_range(aspace,
                                                          addr,
                                                          len,
                                                          &mut residency) {
        return UserRet::from_error(match error {
            // Linux 为区间中存在未映射页返回 ENOMEM。
            MmError::InvalidAddress | MmError::NotMapped => ErrNo::ENOMEM,
            other => mm_err_to_errno(other),
        });
    }
    match copy_to_user(vector, &residency) {
        Ok(copied) if copied == residency.len() => UserRet::from_success(0),
        _ => UserRet::from_error(ErrNo::EFAULT),
    }
}
