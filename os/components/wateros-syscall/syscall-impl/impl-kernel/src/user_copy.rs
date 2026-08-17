//! 用户虚拟地址与内核缓冲区之间的安全拷贝。

//! 本模块代码由AI完成
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use api_v0::ErrNo;
#[cfg(feature = "user-copy-diagnostics")]
use log::{trace, warn};
use mm::api::addr::VirtAddr;
use mm::api::user_access::{FutexMappingIdentity, UserMemoryOps};
use mm::ActiveUserMemoryOps;

use crate::mm_util::{current_user_aspace_handle, mm_err_to_errno};

pub(crate) const USER_PATH_MAX : usize = 4096;
const PATH_COPY_CHUNK : usize = 64;

fn mm_user_copy_errno(e : mm::api::error::MmError) -> ErrNo {
    match e {
        mm::api::error::MmError::InvalidAddress => ErrNo::EFAULT,
        other => mm_err_to_errno(other),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UserWriteProgress {
    pub copied : usize,
    pub error : Option<ErrNo>,
}

// 本方法代码由AI完成
pub(crate) fn user_aspace_required() -> Result<ActiveUserMemoryOps, ErrNo> {
    let handle = current_user_aspace_handle().ok_or(ErrNo::EFAULT)?;
    Ok(ActiveUserMemoryOps::new(handle))
}

// 本方法代码由AI完成
pub(crate) fn copy_from_user(buf : &mut [u8], ptr : usize) -> Result<usize, ErrNo> {
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
           mm_user_copy_errno(e)
       })
}

/// 使用调用方已经捕获的地址空间读取用户内存。
///
/// 适用于持有 scheduler 锁的条件复查路径，避免再次通过 task API 查询当前
/// 地址空间而重入 scheduler 锁。
pub(crate) fn copy_from_user_in_aspace(handle : usize,
                                       buf : &mut [u8],
                                       ptr : usize)
                                       -> Result<usize, ErrNo> {
    if buf.is_empty() {
        return Ok(0);
    }
    if handle == 0 || ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    ActiveUserMemoryOps::new(handle).copy_from_user(buf, VirtAddr(ptr))
                                    // 此函数允许在 scheduler 锁内调用；错误路径也不能查询当前 task。
                                    .map_err(mm_user_copy_errno)
}

pub(crate) fn atomic_load_user_u32_in_aspace(handle : usize, ptr : usize) -> Result<u32, ErrNo> {
    if handle == 0 || ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    ActiveUserMemoryOps::new(handle).atomic_load_u32(VirtAddr(ptr))
                                    .map_err(mm_err_to_errno)
}

pub(crate) fn atomic_compare_exchange_user_u32_in_aspace(handle : usize,
                                                         ptr : usize,
                                                         expected : u32,
                                                         desired : u32)
                                                         -> Result<u32, ErrNo> {
    if handle == 0 || ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    ActiveUserMemoryOps::new(handle).atomic_compare_exchange_u32(VirtAddr(ptr), expected, desired)
                                    .map_err(mm_err_to_errno)
}

pub(crate) fn futex_mapping_identity_u32_in_aspace(handle : usize,
                                                   ptr : usize)
                                                   -> Result<FutexMappingIdentity, ErrNo> {
    if handle == 0 || ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    ActiveUserMemoryOps::new(handle).futex_mapping_identity_u32(VirtAddr(ptr))
                                    .map_err(mm_err_to_errno)
}

// 本方法代码由AI完成
pub(crate) fn copy_to_user(ptr : usize, buf : &[u8]) -> Result<usize, ErrNo> {
    let progress = copy_to_user_progress(ptr, buf);
    match progress.error {
        Some(error) => Err(error),
        None => Ok(progress.copied),
    }
}

pub(crate) fn copy_to_user_progress(ptr : usize, buf : &[u8]) -> UserWriteProgress {
    if buf.is_empty() {
        return UserWriteProgress { copied : 0,
                                   error : None };
    }
    if ptr == 0 {
        return UserWriteProgress { copied : 0,
                                   error : Some(ErrNo::EFAULT) };
    }
    let Some(handle) = current_user_aspace_handle() else {
        return UserWriteProgress { copied : 0,
                                   error : Some(ErrNo::EFAULT) };
    };
    #[cfg(feature = "user-copy-diagnostics")]
    {
        let task_snap = task::current_task_snapshot();
        let task_satp = task_snap.map_or(0, |snap| snap.user_address_space_token);
        let trap_satp = task_snap.map_or(0, |snap| {
                                     snap.trap_return_address_space_token
                                 });
        if handle != 0 && task_satp != 0 && trap_satp != 0 && task_satp != trap_satp {
            warn!("[user-copy] satp mismatch task={:#x} trap={:#x} handle={:#x} va={:#x}",
                  task_satp, trap_satp, handle, ptr);
        }
    }
    let ops = ActiveUserMemoryOps::new(handle);
    let progress = ops.copy_to_user_progress(VirtAddr(ptr), buf);
    let error = progress.error
                        .map(|error| {
                            trace_user_copy_failure("copy_to_user", ptr, buf.len(), error);
                            mm_user_copy_errno(error)
                        });
    UserWriteProgress { copied : progress.copied,
                        error }
}

#[cfg(all(feature = "user-copy-diagnostics", target_arch = "riscv64"))]
fn trace_user_copy_failure(op : &str, va : usize, len : usize, err : mm::api::error::MmError) {
    let handle = current_user_aspace_handle().unwrap_or(0);
    let task_snap = task::current_task_snapshot();
    let task_satp = task_snap.map_or(0, |snap| snap.user_address_space_token);
    let trap_satp = task_snap.map_or(0, |snap| {
                                 snap.trap_return_address_space_token
                             });
    #[cfg(debug_assertions)]
    let probe = mm::user_access::debug_probe_user_virt(handle, VirtAddr(va));
    #[cfg(not(debug_assertions))]
    let probe = "disabled";
    trace!("[user-copy] {op} fail va={va:#x} len={len} err={err:?} handle={handle:#x} \
            task_satp={task_satp:#x} trap_satp={trap_satp:#x} probe={probe:?}");
}

#[cfg(all(feature = "user-copy-diagnostics", target_arch = "loongarch64"))]
fn trace_user_copy_failure(op : &str, va : usize, len : usize, err : mm::api::error::MmError) {
    let handle = current_user_aspace_handle().unwrap_or(0);
    let task_snap = task::current_task_snapshot();
    let task_satp = task_snap.map_or(0, |snap| snap.user_address_space_token);
    let trap_satp = task_snap.map_or(0, |snap| {
                                 snap.trap_return_address_space_token
                             });
    trace!("[user-copy] {op} fail va={va:#x} len={len} err={err:?} handle={handle:#x} \
            task_satp={task_satp:#x} trap_satp={trap_satp:#x}");
    let _ = (handle, task_satp, trap_satp);
}

#[cfg(not(feature = "user-copy-diagnostics"))]
#[inline(always)]
fn trace_user_copy_failure(_op : &str, _va : usize, _len : usize, _err : mm::api::error::MmError) {}

#[cfg(all(feature = "user-copy-diagnostics",
          not(any(target_arch = "riscv64", target_arch = "loongarch64"))))]
fn trace_user_copy_failure(_op : &str, _va : usize, _len : usize, _err : mm::api::error::MmError) {}

/// 读取以 NUL 结尾的用户路径（上限 `max` 字节，含终止符空间）。
// 本方法代码由AI完成
pub(crate) fn copy_user_path_cstr(ptr : usize, max : usize) -> Result<String, ErrNo> {
    if ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    if max == 0 {
        return Err(ErrNo::EINVAL);
    }
    let ops = user_aspace_required()?;
    copy_cstr_in_chunks(ptr, max, |buf, address| {
        ops.copy_from_user(buf, VirtAddr(address))
           .map_err(mm_user_copy_errno)
    })
}

fn copy_cstr_in_chunks(mut ptr : usize,
                       max : usize,
                       mut read : impl FnMut(&mut [u8], usize) -> Result<usize, ErrNo>)
                       -> Result<String, ErrNo> {
    let mut raw = Vec::with_capacity(max.min(256));
    let mut len = 0usize;
    while len < max {
        let chunk_len = (max - len).min(PATH_COPY_CHUNK);
        let mut chunk = [0u8; PATH_COPY_CHUNK];
        match read(&mut chunk[..chunk_len], ptr) {
            Ok(copied) if copied == chunk_len => {
                let end = chunk[..chunk_len].iter().position(|byte| *byte == 0);
                let data_len = end.unwrap_or(chunk_len);
                raw.extend_from_slice(&chunk[..data_len]);
                len += data_len;
                if end.is_some() {
                    break;
                }
            }
            Ok(_) | Err(_) => {
                // A block may straddle an invalid page even when a NUL appears
                // in the valid prefix. Fall back to the byte semantics for this
                // chunk so that the fault boundary remains unchanged.
                for byte_offset in 0..chunk_len {
                    let byte_ptr = ptr.checked_add(byte_offset).ok_or(ErrNo::EFAULT)?;
                    let mut byte = [0u8; 1];
                    read(&mut byte, byte_ptr)?;
                    if byte[0] == 0 {
                        return String::from_utf8(raw).map_err(|_| ErrNo::EINVAL);
                    }
                    raw.push(byte[0]);
                    len += 1;
                }
            }
        }
        ptr = ptr.checked_add(chunk_len).ok_or(ErrNo::EFAULT)?;
    }
    if len >= max {
        return Err(ErrNo::ENAMETOOLONG);
    }
    raw.truncate(len);
    String::from_utf8(raw).map_err(|_| ErrNo::EINVAL)
}

// 本方法代码由AI完成
pub(crate) fn copy_to_user_struct<T : Copy>(ptr : usize, value : &T) -> Result<(), ErrNo> {
    let bytes = unsafe {
        core::slice::from_raw_parts((value as *const T) as *const u8,
                                    core::mem::size_of::<T>())
    };
    copy_to_user(ptr, bytes).map(|n| {
                                if n == bytes.len() {
                                    Ok(())
                                } else {
                                    Err(ErrNo::EFAULT)
                                }
                            })?
}

// 本方法代码由AI完成
pub(crate) fn copy_to_user_struct_in_aspace<T : Copy>(handle : usize,
                                                      ptr : usize,
                                                      value : &T)
                                                      -> Result<(), ErrNo> {
    if ptr == 0 || handle == 0 {
        return Err(ErrNo::EFAULT);
    }
    let bytes = unsafe {
        core::slice::from_raw_parts((value as *const T) as *const u8,
                                    core::mem::size_of::<T>())
    };
    let ops = ActiveUserMemoryOps::new(handle);
    ops.copy_to_user(VirtAddr(ptr), bytes)
       .map_err(|e| {
           trace_user_copy_failure("copy_to_user_in_aspace",
                                   ptr,
                                   bytes.len(),
                                   e);
           mm_user_copy_errno(e)
       })
       .and_then(|n| {
           if n == bytes.len() {
               Ok(())
           } else {
               Err(ErrNo::EFAULT)
           }
       })
}

// 本方法代码由AI完成
pub(crate) fn copy_from_user_struct<T : Copy>(ptr : usize) -> Result<T, ErrNo> {
    let mut value = core::mem::MaybeUninit::<T>::uninit();
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(value.as_mut_ptr() as *mut u8,
                                        core::mem::size_of::<T>())
    };
    let copied = copy_from_user(bytes, ptr)?;
    if copied != bytes.len() {
        return Err(ErrNo::EFAULT);
    }
    Ok(unsafe { value.assume_init() })
}

/// 一次导入连续的用户结构数组，避免逐项重新捕获当前地址空间。
pub(crate) fn copy_from_user_array<T : Copy>(ptr : usize,
                                             count : usize)
                                             -> Result<Vec<T>, ErrNo> {
    if count == 0 {
        return Ok(Vec::new());
    }
    if ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    let element_size = core::mem::size_of::<T>();
    let byte_len = element_size.checked_mul(count).ok_or(ErrNo::EFAULT)?;
    if byte_len > 0 {
        ptr.checked_add(byte_len - 1).ok_or(ErrNo::EFAULT)?;
    }

    let mut values = Vec::<core::mem::MaybeUninit<T>>::new();
    values.try_reserve_exact(count).map_err(|_| ErrNo::ENOMEM)?;
    unsafe {
        values.set_len(count);
    }
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(values.as_mut_ptr() as *mut u8, byte_len)
    };
    let copied = copy_from_user(bytes, ptr)?;
    if copied != byte_len {
        return Err(ErrNo::EFAULT);
    }

    let result = unsafe {
        let result = Vec::from_raw_parts(values.as_mut_ptr() as *mut T,
                                         values.len(),
                                         values.capacity());
        core::mem::forget(values);
        result
    };
    Ok(result)
}

pub(crate) fn copy_from_user_struct_in_aspace<T : Copy>(handle : usize,
                                                        ptr : usize)
                                                        -> Result<T, ErrNo> {
    let mut value = core::mem::MaybeUninit::<T>::uninit();
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(value.as_mut_ptr() as *mut u8,
                                        core::mem::size_of::<T>())
    };
    let copied = copy_from_user_in_aspace(handle, bytes, ptr)?;
    if copied != bytes.len() {
        return Err(ErrNo::EFAULT);
    }
    Ok(unsafe { value.assume_init() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slice_reader<'a>(base : usize,
                        data : &'a [u8],
                        calls : &'a mut usize)
                        -> impl FnMut(&mut [u8], usize) -> Result<usize, ErrNo> + 'a {
        move |dst, address| {
            *calls += 1;
            let offset = address.checked_sub(base).ok_or(ErrNo::EFAULT)?;
            let end = offset.checked_add(dst.len()).ok_or(ErrNo::EFAULT)?;
            let source = data.get(offset..end).ok_or(ErrNo::EFAULT)?;
            dst.copy_from_slice(source);
            Ok(dst.len())
        }
    }

    #[test]
    fn chunked_cstr_reads_one_block_per_chunk() {
        let base = 0x1000;
        let mut data = [b'a'; PATH_COPY_CHUNK * 2 + 1];
        data[PATH_COPY_CHUNK * 2] = 0;
        let mut calls = 0;
        let value = copy_cstr_in_chunks(base,
                                        data.len(),
                                        slice_reader(base, &data, &mut calls)).unwrap();
        assert_eq!(value.len(), PATH_COPY_CHUNK * 2);
        assert_eq!(calls, 3);
    }

    #[test]
    fn chunked_cstr_falls_back_to_nul_before_fault() {
        let base = 0x2000;
        let data = [b'x', 0];
        let mut calls = 0;
        let value = copy_cstr_in_chunks(base,
                                        PATH_COPY_CHUNK,
                                        slice_reader(base, &data, &mut calls)).unwrap();
        assert_eq!(value, "x");
        assert_eq!(calls, 3);
    }

    #[test]
    fn chunked_cstr_rejects_missing_nul() {
        let base = 0x3000;
        let data = [b'x'; PATH_COPY_CHUNK];
        let mut calls = 0;
        assert_eq!(copy_cstr_in_chunks(base,
                                       data.len(),
                                       slice_reader(base, &data, &mut calls)),
                   Err(ErrNo::ENAMETOOLONG));
        assert_eq!(calls, 1);
    }

    #[test]
    fn chunked_cstr_rejects_invalid_utf8() {
        let base = 0x4000;
        let data = [0xff, 0];
        let mut calls = 0;
        assert_eq!(copy_cstr_in_chunks(base,
                                       data.len(),
                                       slice_reader(base, &data, &mut calls)),
                   Err(ErrNo::EINVAL));
        assert_eq!(calls, 1);
    }
}
