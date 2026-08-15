//! 本模块代码由AI完成
//! 将 [`api_v0::kernel_bringup::LoadedElf::user_aspace_ptr`] 解析为 [`Sv39AddressSpace`]，
//! 供上层在闭包内调用 [`api_v0::brk::HeapBrk`] / [`api_v0::mmap::MmapOps`] 等机制原语。
//!
//! **Safety**：`handle` 须来自 bring-up 泄漏的用户地址空间，且与当前任务安装的 `satp` 一致。

use core::sync::atomic::{AtomicBool, Ordering};
use core::sync::atomic::{AtomicU64, AtomicUsize};

use api_v0::address_space::AddressSpaceOps;
use api_v0::error::{MmError, MmResult};
use wateros_base::sync::MultiprocessorSafeCell;
use wateros_base_config::task::MAX_CPUS;

static TLB_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
static TLB_PENDING: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];
static TLB_COMPLETED: [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];
fn debug_cpu_id() -> usize { platform::arch::cpu::current_cpu_id().raw() }

static TLB_SHOOTDOWN_LOCK: debug::TrackedMutex<()> =
    debug::TrackedMutex::new((), debug::DebugLockKind::AddressSpace, debug_cpu_id);

use crate::pagetable::Sv39AddressSpace;

pub(crate) struct UserAddressSpaceCell {
    pub(crate) inner: MultiprocessorSafeCell<Sv39AddressSpace>,
    dropped: AtomicBool,
    /// 所有可能缓存该 ASID TLB 项的 hart；ASID 有效期间只增不减。
    tlb_cpus: AtomicU64,
    token: usize,
}

impl UserAddressSpaceCell {
    pub(crate) fn new(aspace: Sv39AddressSpace) -> Self {
        let token = aspace.satp_value();
        Self { inner: MultiprocessorSafeCell::new(aspace),
               dropped: AtomicBool::new(false),
               tlb_cpus: AtomicU64::new(0),
               token }
    }

    fn is_dropped(&self) -> bool { self.dropped.load(Ordering::Acquire) }

    pub(crate) fn mark_dropped(&self) -> bool {
        !self.dropped.swap(true, Ordering::AcqRel)
    }
}

pub(crate) fn into_handle(aspace: Sv39AddressSpace) -> usize {
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
    let cached = cell.tlb_cpus.swap(0, Ordering::AcqRel);
    let asid = cell.inner
                   .exclusive_access()
                   .destroy_and_take_asid();
    if asid != crate::asid::KERNEL_ASID {
        platform::arch::paging::flush_tlb_local(
            platform::arch::paging::TlbFlushRange::AddressSpace { token : cell.token });
        if request_tlb_shootdown_targets(wateros_base::cpu::CpuMask::from_bits(cached)) {
            crate::asid::release_user(asid);
        } else {
            log::warn!("[tlb] retiring RISC-V ASID {} because shootdown did not complete", asid);
        }
    }
}

pub fn mark_active(handle: usize, cpu: wateros_base::cpu::CpuId) {
    let Some(cell) = (unsafe { cell(handle) }) else { return };
    if cell.is_dropped() || cpu.raw() >= u64::BITS as usize { return; }
    let cpu_bit = 1u64 << cpu.raw();
    let previous = cell.tlb_cpus.fetch_or(cpu_bit, Ordering::AcqRel);
    let asid = crate::asid::from_token(cell.token);
    if previous & cpu_bit == 0 && asid != 0 {
        // 首次在本 hart 使用该 ASID：清除可能由复用遗留的项，同时使页表构造
        // 写入先于后续地址翻译可见。
        platform::arch::paging::flush_tlb_local(
            platform::arch::paging::TlbFlushRange::AddressSpace { token : cell.token });
    }
}

pub fn mark_inactive(_handle: usize, _cpu: wateros_base::cpu::CpuId) {
    // satp 切换不再全量刷新后，hart 离开地址空间并不代表其 TLB 中已无该 ASID。
    // 该位保持到地址空间销毁，保证页表修改与 ASID 回收会通知所有缓存 hart。
}

/// syscall trap 期间全局中断处于关闭状态，不能直接无限自旋等待 shootdown
/// 串行锁。否则两个共享地址空间的 CPU 可形成：
///
/// A 持锁等待 B 的 TLB IPI 确认，B 关中断等待 A 释放锁。
///
/// 等锁期间主动消费本 CPU 已发布的请求，使 A 能完成并释放锁。稍后到达的
/// SSIP 仍会经过正常 trap 路径清除；由于 completed 已推进，不会重复 flush。
fn lock_tlb_shootdown() -> debug::TrackedMutexGuard<'static, ()> {
    let mut reported_wait = false;
    loop {
        if let Some(guard) = TLB_SHOOTDOWN_LOCK.try_lock() {
            return guard;
        }
        if debug::ENABLED && !reported_wait {
            let cpu = platform::arch::cpu::current_cpu_id().raw();
            let (kind, object) = TLB_SHOOTDOWN_LOCK.debug_identity();
            debug::lock_wait(cpu, 0, debug::NO_TASK, kind, object);
            reported_wait = true;
        }
        let _ = handle_tlb_shootdown_ipi();
        core::hint::spin_loop();
    }
}

fn request_tlb_shootdown_targets(mut targets : wateros_base::cpu::CpuMask) -> bool {
    // Serialize sequence allocation, pending publication, IPI delivery and
    // acknowledgements.  A per-CPU pending slot cannot represent two
    // concurrent transactions safely without this critical section.
    let _request_guard = lock_tlb_shootdown();
    let current = platform::arch::cpu::current_cpu_id();
    targets = wateros_base::cpu::CpuMask::from_bits(targets.bits() &
                                                     task::online_cpu_mask().bits());
    targets.remove(current);
    if targets.is_empty() {
        return true;
    }
    match platform::smp::flush_tlb_remote(targets) {
        Ok(()) => return true,
        Err(platform::smp::PlatformSmpError::Unsupported) => {}
        Err(error) => {
            log::warn!("[tlb] platform remote flush failed targets={:#x} error={:?}; \
                        falling back to software IPI",
                       targets.bits(),
                       error);
        }
    }
    let sequence = TLB_SEQUENCE.fetch_add(1, Ordering::AcqRel) + 1;
    let mut raw = targets.bits();
    while raw != 0 {
        let cpu = raw.trailing_zeros() as usize;
        raw &= raw - 1;
        if cpu < MAX_CPUS {
            TLB_PENDING[cpu].store(sequence, Ordering::Release);
        }
    }
    if let Err(error) = platform::smp::send_ipi(targets, platform::smp::IpiKind::TlbShootdown) {
        log::warn!("[tlb] shootdown IPI failed sequence={} targets={:#x} error={:?}",
                   sequence,
                   targets.bits(),
                   error);
        return false;
    }
    let mut completed = true;
    let mut raw = targets.bits();
    while raw != 0 {
        let cpu = raw.trailing_zeros() as usize;
        raw &= raw - 1;
        if cpu >= MAX_CPUS {
            continue;
        }
        let mut spins = 0usize;
        while TLB_COMPLETED[cpu].load(Ordering::Acquire) < sequence && spins < 10_000_000 {
            core::hint::spin_loop();
            spins += 1;
        }
        if spins == 10_000_000 {
            log::warn!("[tlb] shootdown timeout cpu={} sequence={}", cpu, sequence);
            completed = false;
        }
    }
    completed
}

fn request_tlb_shootdown(handle: usize) {
    let cached = unsafe { cell(handle) }
                    .map(|cell| cell.tlb_cpus.load(Ordering::Acquire))
                    .unwrap_or(0);
    let _ = request_tlb_shootdown_targets(wateros_base::cpu::CpuMask::from_bits(cached));
}

pub fn handle_tlb_shootdown_ipi() -> bool {
    let cpu = platform::arch::cpu::current_cpu_id().raw();
    if cpu >= MAX_CPUS {
        return false;
    }
    let pending = TLB_PENDING[cpu].load(Ordering::Acquire);
    let completed = TLB_COMPLETED[cpu].load(Ordering::Relaxed);
    if pending <= completed {
        return false;
    }
    platform::arch::paging::flush_tlb_local(platform::arch::paging::TlbFlushRange::All);
    TLB_COMPLETED[cpu].store(pending, Ordering::Release);
    true
}

#[inline]
/// 在有效用户地址空间上执行 `f`；`handle == 0` 返回 [`MmError::InvalidAddress`]。
pub fn with_user_aspace_mut<R>(
    handle: usize,
    f: impl FnOnce(&mut Sv39AddressSpace) -> MmResult<R>,
) -> MmResult<R> {
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

pub fn with_user_aspace_mut_and_flush<R>(
    handle: usize,
    f: impl FnOnce(&mut Sv39AddressSpace) -> MmResult<R>,
) -> MmResult<R> {
    let result = with_user_aspace_mut(handle, f);
    platform::arch::paging::flush_tlb_local(platform::arch::paging::TlbFlushRange::All);
    request_tlb_shootdown(handle);
    result
}

/// Run `f` and invalidate the address space only when it reports a PTE change.
/// Errors retain the old conservative flush because `f` may have changed an
/// earlier PTE before discovering an invalid page later in the range.
pub fn with_user_aspace_mut_and_flush_if_changed<R>(
    handle: usize,
    f: impl FnOnce(&mut Sv39AddressSpace) -> MmResult<(R, bool)>,
) -> MmResult<R> {
    match with_user_aspace_mut(handle, f) {
        Ok((value, false)) => Ok(value),
        Ok((value, true)) => {
            platform::arch::paging::flush_tlb_local(platform::arch::paging::TlbFlushRange::All);
            request_tlb_shootdown(handle);
            Ok(value)
        }
        Err(error) => {
            platform::arch::paging::flush_tlb_local(platform::arch::paging::TlbFlushRange::All);
            request_tlb_shootdown(handle);
            Err(error)
        }
    }
}

/// Run `f`, always invalidate the faulting page locally, and notify other CPUs
/// only when `f` reports that a PTE changed.
///
/// The unconditional local invalidation is required for a shared address
/// space: another CPU may already have resolved the COW PTE and completed the
/// remote shootdown while this CPU was entering the same store fault.  In that
/// case the page table is writable, but this hart can still hold the stale
/// read-only translation which caused the trap.
pub fn with_user_aspace_mut_and_page_flush<R>(
    handle: usize,
    page: usize,
    f: impl FnOnce(&mut Sv39AddressSpace) -> MmResult<(R, bool)>,
) -> MmResult<R> {
    let (value, changed) = with_user_aspace_mut(handle, f)?;
    platform::arch::paging::flush_tlb_local(
        platform::arch::paging::TlbFlushRange::Page { addr: page },
    );
    if changed {
        request_tlb_shootdown(handle);
    }
    Ok(value)
}
