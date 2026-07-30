//! 堆分配路径的中断屏蔽与递归检测。
//!
//! **不变量**：同一调用栈上不得嵌套进入 `GlobalAlloc`；高水位告警每个引导周期至多一次。

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use base::cpu::CpuLocal;
use config::mm::KERNEL_HEAP_SIZE;
use config::task::MAX_CPUS;

/// 每 CPU 的 allocator 进入深度。
///
/// ALLOC_SYNC: 在关中断前增加深度，以捕获 logger/allocator 等同步路径上的递归分配；
/// 它不是全局锁，跨 CPU 互斥由具体 allocator backend 的锁负责。
static HEAP_GUARD_DEPTH : CpuLocal<AtomicUsize, MAX_CPUS> =
    CpuLocal::from_cells([const { UnsafeCell::new(AtomicUsize::new(0)) }; MAX_CPUS]);
/// 高水位日志只打印一次，避免 OOM 前的每次 alloc 都放大串口输出压力。
static HEAP_HIGH_WATER_WARNED : AtomicBool = AtomicBool::new(false);

const HEAP_HIGH_WATER_NUMERATOR : usize = 9;
const HEAP_HIGH_WATER_DENOMINATOR : usize = 10;

/// 关本 CPU 全局中断后执行 `f`；检测递归分配并 panic。
///
/// 读取中断状态失败时 panic，避免“已 disable 但无法 restore、中断永久关闭”。
/// `f` 不得触发调度、等待、VFS 回调或日志格式化分配；否则会在本 guard 内死锁或 panic。
pub(crate) fn with_allocator_interrupt_guard<R>(f : impl FnOnce() -> R) -> R {
    let cpu = arch::cpu::current_cpu_id();
    let local_depth = HEAP_GUARD_DEPTH.get(cpu).unwrap_or_else(|| {
                                                    panic!("heap guard: invalid CPU id {}",
                                                           cpu.raw())
                                                });
    let depth = local_depth.fetch_add(1, Ordering::Acquire);
    if depth > 0 {
        local_depth.fetch_sub(1, Ordering::Release);
        panic!("recursive heap allocation detected (cpu={} depth={})",
               cpu.raw(),
               depth + 1);
    }
    let state = arch::interrupt::read_global_interrupt_state()
                    .expect("heap guard: read interrupt state");
    let _ = arch::interrupt::disable_global_interrupt();
    let ret = f();
    let _ = arch::interrupt::restore_global_interrupt_state(state);
    local_depth.fetch_sub(1, Ordering::Release);
    ret
}

/// 已用堆超过容量 90% 时打印一次 warn。
pub(crate) fn maybe_warn_high_water(used : usize, free : usize) {
    if HEAP_HIGH_WATER_WARNED.load(Ordering::Relaxed) {
        return;
    }
    if used > KERNEL_HEAP_SIZE * HEAP_HIGH_WATER_NUMERATOR / HEAP_HIGH_WATER_DENOMINATOR {
        HEAP_HIGH_WATER_WARNED.store(true, Ordering::Relaxed);
        log::warn!("[heap] high water: used={} free={} cap={}",
                   used,
                   free,
                   KERNEL_HEAP_SIZE);
    }
}
