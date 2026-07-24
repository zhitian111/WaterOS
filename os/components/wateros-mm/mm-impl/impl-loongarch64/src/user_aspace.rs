//! 本模块代码由AI完成
//! 将 [`api_v0::kernel_bringup::LoadedElf::user_aspace_ptr`] 解析为
//! [`LoongArch64AddressSpace`]， 供上层在闭包内调用 [`api_v0::brk::HeapBrk`] /
//! [`api_v0::mmap::MmapOps`] 等机制原语。
//!
//! **Safety**：`handle` 须来自 bring-up 泄漏的用户地址空间，且与当前任务安装的
//! PGDL 一致。

use core::sync::atomic::{AtomicBool, Ordering};
use core::sync::atomic::AtomicUsize;

use api_v0::error::{MmError, MmResult};
use wateros_base::sync::MultiprocessorSafeCell;
use wateros_base_config::task::MAX_CPUS;
use spin::Mutex;

static TLB_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
static TLB_PENDING: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];
static TLB_COMPLETED: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];
static TLB_SHOOTDOWN_LOCK: Mutex<()> = Mutex::new(());

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

fn request_tlb_shootdown() {
    // Serialize sequence allocation, pending publication, IPI delivery and
    // acknowledgements.  A per-CPU pending slot cannot represent two
    // concurrent transactions safely without this critical section.
    let _request_guard = TLB_SHOOTDOWN_LOCK.lock();
    let current = platform::arch::cpu::current_cpu_id();
    // Stage one invalidates every online CPU.  Restricting this to online
    // CPUs avoids waiting for configured-but-not-yet-started harts; tracking
    // the CPUs actively using this address space is a later optimization.
    let mut targets = task::online_cpu_mask();
    targets.remove(current);
    if targets.is_empty() { return; }
    let sequence = TLB_SEQUENCE.fetch_add(1, Ordering::AcqRel) + 1;
    let mut raw = targets.bits();
    while raw != 0 {
        let cpu = raw.trailing_zeros() as usize;
        raw &= raw - 1;
        if cpu < MAX_CPUS { TLB_PENDING[cpu].store(sequence, Ordering::Release); }
    }
    if let Err(error) = platform::smp::send_ipi(targets) {
        log::warn!("[tlb] shootdown IPI failed sequence={} targets={:#x} error={:?}",
                   sequence,
                   targets.bits(),
                   error);
        return;
    }
    let mut raw = targets.bits();
    while raw != 0 {
        let cpu = raw.trailing_zeros() as usize;
        raw &= raw - 1;
        if cpu >= MAX_CPUS { continue; }
        let mut spins = 0usize;
        while TLB_COMPLETED[cpu].load(Ordering::Acquire) < sequence && spins < 10_000_000 {
            core::hint::spin_loop();
            spins += 1;
        }
        if spins == 10_000_000 {
            log::warn!("[tlb] shootdown timeout cpu={} sequence={}", cpu, sequence);
        }
    }
}

pub fn handle_tlb_shootdown_ipi() -> bool {
    let cpu = platform::arch::cpu::current_cpu_id().raw();
    if cpu >= MAX_CPUS { return false; }
    let pending = TLB_PENDING[cpu].load(Ordering::Acquire);
    if pending <= TLB_COMPLETED[cpu].load(Ordering::Relaxed) { return false; }
    platform::arch::paging::flush_tlb_local(platform::arch::paging::TlbFlushRange::All);
    TLB_COMPLETED[cpu].store(pending, Ordering::Release);
    true
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

pub fn with_user_aspace_mut_and_flush<R>(handle : usize,
                                         f : impl FnOnce(&mut LoongArch64AddressSpace) -> MmResult<R>)
                                         -> MmResult<R> {
    let result = with_user_aspace_mut(handle, f);
    platform::arch::paging::flush_tlb_local(platform::arch::paging::TlbFlushRange::All);
    request_tlb_shootdown();
    result
}
