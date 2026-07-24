//! 本模块代码由AI完成
//! 将 [`api_v0::kernel_bringup::LoadedElf::user_aspace_ptr`] 解析为
//! [`LoongArch64AddressSpace`]， 供上层在闭包内调用 [`api_v0::brk::HeapBrk`] /
//! [`api_v0::mmap::MmapOps`] 等机制原语。
//!
//! **Safety**：`handle` 须来自 bring-up 泄漏的用户地址空间，且与当前任务安装的
//! PGDL 一致。

use core::sync::atomic::{AtomicBool, Ordering};

use api_v0::error::{MmError, MmResult};
use wateros_base::sync::MultiprocessorSafeCell;

use crate::pagetable::LoongArch64AddressSpace;

pub(crate) struct UserAddressSpaceCell {
    pub(crate) inner: MultiprocessorSafeCell<LoongArch64AddressSpace>,
    dropped: AtomicBool,
}

impl UserAddressSpaceCell {
    pub(crate) fn new(aspace: LoongArch64AddressSpace) -> Self {
        Self { inner: MultiprocessorSafeCell::new(aspace),
               dropped: AtomicBool::new(false) }
    }

    fn is_dropped(&self) -> bool { self.dropped.load(Ordering::Acquire) }

    pub(crate) fn mark_dropped(&self) -> bool {
        !self.dropped.swap(true, Ordering::AcqRel)
    }
}

pub(crate) fn into_handle(aspace: LoongArch64AddressSpace) -> usize {
    alloc::boxed::Box::into_raw(alloc::boxed::Box::new(UserAddressSpaceCell::new(aspace))) as usize
}

unsafe fn cell(handle: usize) -> Option<&'static UserAddressSpaceCell> {
    (handle != 0).then(|| unsafe { &*(handle as *const UserAddressSpaceCell) })
}

pub(crate) fn destroy(handle: usize) {
    let Some(cell) = (unsafe { cell(handle) }) else { return };
    if !cell.mark_dropped() {
        return;
    }
    cell.inner.exclusive_access().destroy();
}

#[inline]
/// 在有效用户地址空间上执行 `f`；`handle == 0` 返回
/// [`MmError::InvalidAddress`]。
pub fn with_user_aspace_mut<R>(handle : usize,
                               f : impl FnOnce(&mut LoongArch64AddressSpace) -> MmResult<R>)
                               -> MmResult<R> {
    let cell = unsafe { cell(handle) }.ok_or(MmError::InvalidAddress)?;
    if cell.is_dropped() {
        return Err(MmError::InvalidAddress);
    }
    let mut guard = cell.inner.exclusive_access();
    if cell.is_dropped() {
        return Err(MmError::InvalidAddress);
    }
    f(&mut guard)
}
