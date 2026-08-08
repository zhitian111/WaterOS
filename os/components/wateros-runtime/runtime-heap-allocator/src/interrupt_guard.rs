//! 堆分配路径的中断屏蔽与递归检测。
//!
//! **不变量**：同一调用栈上不得嵌套进入 `GlobalAlloc`；高水位告警每个引导周期至多一次。

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use base::cpu::CpuId;
use config::mm::KERNEL_HEAP_SIZE;
use config::task::MAX_CPUS;

/// 每 CPU 的 allocator 进入深度。
///
/// ALLOC_SYNC: 关中断后增加深度，以捕获 logger/allocator 等同步路径上的递归分配；
/// 它不是全局锁，跨 CPU 互斥由具体 allocator backend 的锁负责。
///
/// 这些 hot guard 静态放在带前后金丝雀的 `HeapGuardCells` 里（`.data` 段），
/// 避免挤在 `.bss` 末尾被相邻越界写破坏，也便于在递归 panic 时校验金丝雀，区分
/// “真递归”与“guard 静态被内存破坏”。
/// 高水位日志只打印一次，避免 OOM 前的每次 alloc 都放大串口输出压力。
static HEAP_HIGH_WATER_WARNED : AtomicBool = AtomicBool::new(false);
/// 当前“正在执行中”的 allocator guard 的调用点返回地址；guard 退出时清零。
///
/// 若递归 panic 时该值非零，即上次 guard 进入后未正常退出（调度/切换跳过了
/// `fetch_sub`/清零），配合 `addr2line` 可直接定位“谁在 guard 内把执行流切走”。
///
/// `HeapGuardCells`：depth/active_ra 数组 + 前后金丝雀，repr(C) 保证相邻。
/// 非零哨兵须放 `.data`（PROGBITS）而非 BSS。
#[repr(C)]
struct HeapGuardCells {
    pre_canary : AtomicUsize,
    depth : [AtomicUsize; MAX_CPUS],
    active_ra : [AtomicUsize; MAX_CPUS],
    post_canary : AtomicUsize,
}
/// 非零金丝雀哨兵。
const GUARD_CANARY : usize = 0xCAFE_BEEF_DEAD_5EED;
#[unsafe(link_section = ".data.scheduler")]
static HEAP_GUARD_CELLS : HeapGuardCells =
    HeapGuardCells { pre_canary : AtomicUsize::new(GUARD_CANARY),
                     depth : [const { AtomicUsize::new(0) }; MAX_CPUS],
                     active_ra : [const { AtomicUsize::new(0) }; MAX_CPUS],
                     post_canary : AtomicUsize::new(GUARD_CANARY) };

/// 本核 allocator 深度槽（越界 CPU id 直接 panic）。
fn depth_slot(cpu : CpuId) -> &'static AtomicUsize {
    HEAP_GUARD_CELLS.depth
                    .get(cpu.index())
                    .unwrap_or_else(|| {
                        panic!("heap guard: invalid CPU id {}",
                               cpu.raw())
                    })
}
/// 本核活跃 guard 调用点槽。
fn active_slot(cpu : CpuId) -> &'static AtomicUsize {
    HEAP_GUARD_CELLS.active_ra
                    .get(cpu.index())
                    .unwrap_or_else(|| {
                        panic!("heap guard: invalid CPU id {}",
                               cpu.raw())
                    })
}
/// guard 数组金丝雀是否完好（pre/post 哨兵未被相邻越界写覆盖）。
fn guard_canaries_ok() -> bool {
    HEAP_GUARD_CELLS.pre_canary
                    .load(Ordering::Relaxed) ==
    GUARD_CANARY &&
    HEAP_GUARD_CELLS.post_canary
                    .load(Ordering::Relaxed) ==
    GUARD_CANARY
}

const HEAP_HIGH_WATER_NUMERATOR : usize = 9;
const HEAP_HIGH_WATER_DENOMINATOR : usize = 10;

/// 读取调用者返回地址（本函数返回后要执行的下一条指令 = 调用点）。
/// 仅 RISC-V 提供有效值；其它架构返回 0（诊断降级，不影响功能）。
///
/// **关键**：必须 `#[inline(always)]` 并作为 `with_allocator_interrupt_guard`
/// 的**第一条语句**调用，否则中间的任何 `jalr` 都会改写 `ra` 导致捕获错误地址。
#[inline(always)]
#[cfg(target_arch = "riscv64")]
fn read_ra() -> usize {
    let r : usize;
    // SAFETY: 纯读 ra 寄存器，无副作用。
    unsafe {
        core::arch::asm!("mv {0}, ra",
                         out(reg) r,
                         options(nomem, nostack));
    }
    r
}

/// 非 RISC-V 架构的诊断降级：返回 0。
#[inline]
#[cfg(not(target_arch = "riscv64"))]
fn read_ra() -> usize { 0 }

/// 读取当前 hart 的帧指针（s0/x8），用于栈回溯。
/// 仅 RISC-V；其它架构返回 0。
#[inline(always)]
#[cfg(target_arch = "riscv64")]
fn read_fp() -> usize {
    let fp : usize;
    unsafe {
        core::arch::asm!("mv {0}, s0", out(reg) fp, options(nomem, nostack));
    }
    fp
}
#[inline]
#[cfg(not(target_arch = "riscv64"))]
fn read_fp() -> usize { 0 }

/// 栈回溯：从当前帧指针向上走 `N` 帧收集返回地址。
/// 依赖帧指针链（需编译时保留 fp）；优化构建可能省略 fp，此时输出全零。
#[cfg(target_arch = "riscv64")]
fn capture_backtrace<const N: usize>() -> [usize; N] {
    let mut bt = [0usize; N];
    let mut fp = read_fp();
    for i in 0..N {
        if fp < 0x8000_0000 || fp > 0x9000_0000 {
            break;
        }
        let saved_ra_ptr = (fp as *const usize).wrapping_sub(1);
        if (saved_ra_ptr as usize) < 0x8000_0000 {
            break;
        }
        bt[i] = unsafe { saved_ra_ptr.read_volatile() };
        fp = unsafe { (fp as *const usize).read_volatile() };
    }
    bt
}
#[cfg(not(target_arch = "riscv64"))]
fn capture_backtrace<const N: usize>() -> [usize; N] { [0usize; N] }

/// 关本 CPU 全局中断后执行 `f`；检测递归分配并 panic。
///
/// 读取中断状态失败时 panic，避免“已 disable 但无法 restore、中断永久关闭”。
/// `f` 不得触发调度、等待、VFS 回调或日志格式化分配；否则会在本 guard 内死锁或 panic。
pub(crate) fn with_allocator_interrupt_guard<R>(f : impl FnOnce() -> R) -> R {
    // 第一条语句：此时 ra 仍是本函数的调用者地址。
    let caller_ra = read_ra();
    let cpu = arch::cpu::current_cpu_id();
    let local_depth = depth_slot(cpu);
    let state =
        arch::interrupt::read_global_interrupt_state().expect("heap guard: read interrupt state");
    let _ = arch::interrupt::disable_global_interrupt();
    let depth = local_depth.fetch_add(1, Ordering::Acquire);
    if depth > 0 {
        // 在关中断状态下捕获所有诊断值，保证一致性。
        let active_ra = active_slot(cpu).load(Ordering::Relaxed);
        let mut depths = [0usize; MAX_CPUS];
        for (i, slot) in depths.iter_mut()
                               .enumerate()
        {
            *slot = HEAP_GUARD_CELLS.depth[i].load(Ordering::Relaxed);
        }
        let canary_ok = guard_canaries_ok();
        let bt = capture_backtrace::<6>();
        // 递减后恢复中断，让 panic 打印能正常输出。
        local_depth.fetch_sub(1, Ordering::Release);
        let _ = arch::interrupt::restore_global_interrupt_state(state);
        panic!("recursive heap allocation detected (cpu={} depth={} active_ra={:#x} cur_ra={:#x} \
                canary_ok={} pre={:#x} post={:#x} depths={:?} bt={:#x?})",
               cpu.raw(),
               depth + 1,
               active_ra,
               caller_ra,
               canary_ok,
               HEAP_GUARD_CELLS.pre_canary
                               .load(Ordering::Relaxed),
               HEAP_GUARD_CELLS.post_canary
                               .load(Ordering::Relaxed),
               &depths[..],
               &bt[..]);
    }
    // 成功进入：用最初捕获的 caller_ra 记录调用点；退出时清零。
    active_slot(cpu).store(caller_ra, Ordering::Relaxed);
    let ret = f();
    local_depth.fetch_sub(1, Ordering::Release);
    active_slot(cpu).store(0, Ordering::Relaxed);
    let _ = arch::interrupt::restore_global_interrupt_state(state);
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
