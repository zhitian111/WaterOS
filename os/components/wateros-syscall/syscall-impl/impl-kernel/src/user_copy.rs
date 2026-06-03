//! 用户虚拟地址与内核缓冲区之间的安全拷贝。

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use abi::errno::ErrNo;
use log::{trace, warn};
use mm::api::addr::VirtAddr;
use mm::api::user_access::UserMemoryOps;
use mm::ActiveUserMemoryOps;

use crate::mm_util::{current_user_aspace_handle, mm_err_to_errno};

pub(crate) fn user_aspace_required() -> Result<ActiveUserMemoryOps, ErrNo> {
    let handle = current_user_aspace_handle().ok_or(ErrNo::EFAULT)?;
    Ok(ActiveUserMemoryOps::new(handle))
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
        .map_err(|e| {
            trace_user_copy_failure("copy_from_user", ptr, buf.len(), e);
            mm_err_to_errno(e)
        })
}

pub(crate) fn copy_to_user(ptr: usize, buf: &[u8]) -> Result<usize, ErrNo> {
    if buf.is_empty() {
        return Ok(0);
    }
    if ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    let handle = current_user_aspace_handle().ok_or(ErrNo::EFAULT)?;
    let task_satp = task::current_task_user_address_space_token();
    let trap_satp = task::current_task_trap_return_address_space_token();
    if handle != 0 && task_satp != 0 && trap_satp != 0 && task_satp != trap_satp {
        warn!("[user-copy] satp mismatch task={:#x} trap={:#x} handle={:#x} va={:#x}",
              task_satp,
              trap_satp,
              handle,
              ptr);
    }
    let ops = ActiveUserMemoryOps::new(handle);
    ops.copy_to_user(VirtAddr(ptr), buf)
        .map_err(|e| {
            trace_user_copy_failure("copy_to_user", ptr, buf.len(), e);
            mm_err_to_errno(e)
        })
}

#[cfg(target_arch = "riscv64")]
fn trace_user_copy_failure(op: &str, va: usize, len: usize, err: mm::api::error::MmError) {
    let handle = current_user_aspace_handle().unwrap_or(0);
    let task_satp = task::current_task_user_address_space_token();
    let trap_satp = task::current_task_trap_return_address_space_token();
    let probe = mm::user_access::debug_probe_user_virt(handle, VirtAddr(va));
    trace!("[user-copy] {op} fail va={va:#x} len={len} err={err:?} handle={handle:#x} \
            task_satp={task_satp:#x} trap_satp={trap_satp:#x} probe={probe:?}");
}

#[cfg(target_arch = "loongarch64")]
fn trace_user_copy_failure(op: &str, va: usize, len: usize, err: mm::api::error::MmError) {
    let handle = current_user_aspace_handle().unwrap_or(0);
    let task_satp = task::current_task_user_address_space_token();
    let trap_satp = task::current_task_trap_return_address_space_token();
    trace!("[user-copy] {op} fail va={va:#x} len={len} err={err:?} handle={handle:#x} \
            task_satp={task_satp:#x} trap_satp={trap_satp:#x}");
    let _ = (handle, task_satp, trap_satp);
}

#[cfg(not(any(target_arch = "riscv64", target_arch = "loongarch64")))]
fn trace_user_copy_failure(_op: &str, _va: usize, _len: usize, _err: mm::api::error::MmError) {}

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
        ops.copy_from_user(&mut byte, VirtAddr(ptr + len))
            .map_err(mm_err_to_errno)?;
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

pub(crate) fn copy_from_user_struct<T: Copy>(ptr: usize) -> Result<T, ErrNo> {
    let mut value = core::mem::MaybeUninit::<T>::uninit();
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(
            value.as_mut_ptr() as *mut u8,
            core::mem::size_of::<T>(),
        )
    };
    let copied = copy_from_user(bytes, ptr)?;
    if copied != bytes.len() {
        return Err(ErrNo::EFAULT);
    }
    Ok(unsafe { value.assume_init() })
}
