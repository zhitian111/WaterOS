//! 用户虚拟地址与内核缓冲区之间的安全拷贝。

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use abi::errno::ErrNo;
use mm::api::addr::VirtAddr;
use mm::api::user_access::UserMemoryOps;
use mm::user_access::Sv39UserMemoryOps;

use crate::mm_util::{current_user_aspace_handle, mm_err_to_errno};

pub(crate) fn user_aspace_required() -> Result<Sv39UserMemoryOps, ErrNo> {
    let handle = current_user_aspace_handle().ok_or(ErrNo::EFAULT)?;
    Ok(Sv39UserMemoryOps::new(handle))
}

pub(crate) fn copy_from_user(buf: &mut [u8], ptr: usize) -> Result<usize, ErrNo> {
    if buf.is_empty() {
        return Ok(0);
    }
    if ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    let ops = user_aspace_required()?;
    ops.copy_from_user(buf, VirtAddr(ptr))
        .map_err(mm_err_to_errno)
}

pub(crate) fn copy_to_user(ptr: usize, buf: &[u8]) -> Result<usize, ErrNo> {
    if buf.is_empty() {
        return Ok(0);
    }
    if ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    let ops = user_aspace_required()?;
    ops.copy_to_user(VirtAddr(ptr), buf)
        .map_err(mm_err_to_errno)
}

/// 读取以 NUL 结尾的用户路径（上限 `max` 字节，含终止符空间）。
pub(crate) fn copy_user_path_cstr(ptr: usize, max: usize) -> Result<String, ErrNo> {
    if ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    if max == 0 {
        return Err(ErrNo::EINVAL);
    }
    let mut raw = Vec::with_capacity(max.min(256));
    raw.resize(max, 0);
    let ops = user_aspace_required()?;
    let mut len = 0usize;
    while len < max {
        let mut byte = [0u8; 1];
        ops.copy_from_user(&mut byte, VirtAddr(ptr + len)).map_err(mm_err_to_errno)?;
        if byte[0] == 0 {
            break;
        }
        raw[len] = byte[0];
        len += 1;
    }
    if len >= max {
        return Err(ErrNo::ENAMETOOLONG);
    }
    raw.truncate(len);
    String::from_utf8(raw).map_err(|_| ErrNo::EINVAL)
}

pub(crate) fn copy_to_user_struct<T: Copy>(ptr: usize, value: &T) -> Result<(), ErrNo> {
    let bytes = unsafe {
        core::slice::from_raw_parts(
            (value as *const T) as *const u8,
            core::mem::size_of::<T>(),
        )
    };
    copy_to_user(ptr, bytes).map(|n| {
        if n == bytes.len() {
            Ok(())
        } else {
            Err(ErrNo::EFAULT)
        }
    })?
}
