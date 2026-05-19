//! 将 [`api_v0::kernel_bringup::LoadedElf::user_aspace_ptr`] 解析为 [`Sv39AddressSpace`]，
//! 供上层在闭包内调用 [`api_v0::brk::HeapBrk`] / [`api_v0::mmap::MmapOps`] 等机制原语。
//!
//! **Safety**：`handle` 须来自 bring-up 泄漏的用户地址空间，且与当前任务安装的 `satp` 一致。

use api_v0::error::{MmError, MmResult};

use crate::pagetable::Sv39AddressSpace;

#[inline]
unsafe fn aspace_mut(handle: usize) -> Option<&'static mut Sv39AddressSpace> {
    if handle == 0 {
        return None;
    }
    Some(unsafe { &mut *(handle as *mut Sv39AddressSpace) })
}

/// 在有效用户地址空间上执行 `f`；`handle == 0` 返回 [`MmError::InvalidAddress`]。
pub fn with_user_aspace_mut<R>(
    handle: usize,
    f: impl FnOnce(&mut Sv39AddressSpace) -> MmResult<R>,
) -> MmResult<R> {
    let a = unsafe { aspace_mut(handle).ok_or(MmError::InvalidAddress)? };
    f(a)
}
