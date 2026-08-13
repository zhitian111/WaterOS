//! 平台 SMP 聚合接口。
//!
//! 这里不直接操作 SBI、IOCSR 等硬件寄存器；这些由当前 `platform-impl`
//! 的 [`SmpImpl`] 完成。本模块只保存跨架构通用的待处理 IPI reason，确保
//! reason 在通知目标 CPU 前已经发布。

pub use api_v0::smp::{HartStatus, IpiKind, PlatformSmp, PlatformSmpError, PlatformSmpResult};

use base::cpu::{CpuId, CpuMask};
use config::task::MAX_CPUS;
use core::sync::atomic::{AtomicU8, Ordering};

/// 每 CPU 的软件 IPI 原因位图。
///
/// IPI_SYNC: 发送方必须先以 `Release` 发布 reason，才调用 profile 的 `send_ipi`；
/// 接收方在 trap 中以 `AcqRel` 取走全部位，因而合并多个发送方的通知而不丢 reason。
/// 该数组只存“要做什么”，不表示 CPU online，也不替代 scheduler 的 need-resched 位。
static PENDING_IPI: [AtomicU8; MAX_CPUS] = [const { AtomicU8::new(0) }; MAX_CPUS];

/// 请求固件/板级后端启动一个辅助 CPU。
///
/// BOOT_CONTRACT: 调用方负责提供该 CPU 的 arch 启动入口与 opaque 参数，并在成功后
/// 等待 OS 自己发布 online；SBI/HSM 返回成功不等于 AP 已完成内核初始化。
#[inline]
pub fn start_cpu(cpu: CpuId, start_addr: usize, opaque: usize) -> PlatformSmpResult<()> {
    crate::active_impl::smp::SmpImpl::start_cpu(cpu, start_addr, opaque)
}

/// 查询固件或板级后端看到的 hart 生命周期状态。
///
/// 这是诊断信息，不能取代 scheduler 的 online mask；后者表示 WaterOS 是否已可向该
/// CPU 投递任务。
#[inline]
pub fn cpu_status(cpu: CpuId) -> PlatformSmpResult<HartStatus> {
    crate::active_impl::smp::SmpImpl::cpu_status(cpu)
}

/// 返回当前 machine 配置允许寻址的逻辑 CPU 集合。
///
/// 该集合不是 online mask，可能包含尚未启动、启动失败或不被 OS 接管的 CPU。
#[inline]
pub fn configured_cpu_mask() -> CpuMask {
    crate::active_impl::smp::SmpImpl::configured_cpu_mask()
}

/// 向 `mask` 中的 CPU 发布 `kind`，然后请求平台发送软件中断。
///
/// IPI_SYNC: 本函数不会筛选 offline CPU；scheduler/调用方必须先以自己的 online
/// 状态过滤目标。发送失败时 reason 会保留，调用方应按自身语义决定重试或撤销。
/// 请求平台执行远端地址翻译缓存刷新。
///
/// 该函数只执行硬件/固件运输；页表锁、active CPU mask、ack 与物理页回收顺序均由
/// MM 层维护。
#[inline]
pub fn send_ipi(mask: CpuMask, kind: IpiKind) -> PlatformSmpResult<()> {
    if debug::ENABLED {
        let sender = crate::arch::cpu::current_cpu_id().raw();
        debug::update_cpu_state(sender, |state| {
            state.ipi_sent = state.ipi_sent.wrapping_add(1);
        });
        debug::record_event(sender,
                            0,
                            debug::NO_TASK,
                            if kind.bits() & IpiKind::TlbShootdown.bits() != 0 {
                                debug::DebugEventKind::TlbShootdown
                            } else {
                                debug::DebugEventKind::IpiSend
                            },
                            0,
                            [mask.bits() as u64, kind.bits() as u64, 0]);
    }
    let mut raw = mask.bits();
    while raw != 0 {
        let cpu = raw.trailing_zeros() as usize;
        raw &= raw - 1;
        if cpu < MAX_CPUS {
            PENDING_IPI[cpu].fetch_or(kind.bits(), Ordering::Release);
        }
    }
    crate::active_impl::smp::SmpImpl::send_ipi(mask)
}

#[inline]
pub fn flush_tlb_remote(mask: CpuMask) -> PlatformSmpResult<()> {
    crate::active_impl::smp::SmpImpl::flush_tlb_remote(mask)
}

/// 请求平台同步刷新所选 CPU 的指令缓存。
#[inline]
pub fn flush_icache_remote(mask: CpuMask) -> PlatformSmpResult<()> {
    crate::active_impl::smp::SmpImpl::flush_icache_remote(mask)
}

/// 取走当前 CPU 已累积的全部 IPI 原因。
///
/// 仅能在当前 CPU 的软件中断处理路径调用；普通路径抢先取走会使 trap 看不到通知。
#[inline]
pub fn take_pending_ipi(cpu: CpuId) -> u8 {
    if cpu.raw() >= MAX_CPUS {
        return 0;
    }
    PENDING_IPI[cpu.raw()].swap(0, Ordering::AcqRel)
}

#[inline]
pub fn init_ipi() -> PlatformSmpResult<()> {
    crate::active_impl::smp::SmpImpl::init_ipi()
}

/// 清除当前 CPU 已触发的硬件软件中断。
///
/// 这是本地 ISA 寄存器操作，不经过 OpenSBI、IOCSR mailbox 等 platform IPI
/// 运输层；必须在 trap 返回前完成。
#[inline]
pub fn clear_ipi() {
    crate::arch::interrupt::clear_soft_interrupt();
}
