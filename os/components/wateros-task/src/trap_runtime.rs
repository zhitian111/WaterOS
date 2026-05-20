//! Trap 与任务运行时的 **Rust 具名入口**（供 `wateros-riscv64-trap` 等上层 crate 依赖调用）。
//!
//! 与 `#[no_mangle] extern "C"` 符号的对应关系见 [`crate::runtime`]：汇编或其它 ABI 仍通过 C 符号转发到本模块，避免在平台 arch 实现里写匿名 `extern "C"` 块。

use crate::scheduler;
use crate::scheduler::TaskTrapFrame;
use arch::paging;
use core::sync::atomic::{AtomicUsize, Ordering};

// 由 `init_kernel_trap_satp` 在页表就绪后写入；`install_satp_for_exception_return` 在返回内核态时读回。使用 `Release`/`Relaxed` 与单次初始化路径匹配。
static KERNEL_TRAP_SATP: AtomicUsize = AtomicUsize::new(0);

/// 在全局内核页表 `satp` 就绪后注册，供 trap 在返回内核态时写回。
#[inline]
pub fn init_kernel_trap_satp(v: usize) {
    KERNEL_TRAP_SATP.store(v, Ordering::Release);
}

/// 在 [`wateros_kernel_trap_handler`] 内、确认 trap 来自用户态后切换到全局内核 `satp`。
///
/// trap 汇编与 `trap_entry_rust` 仍运行在用户 `satp`（用户表内 RAM 恒等映射）；syscall / FS /
/// 驱动等 handler 体内逻辑在此之后使用内核页表（含 MMIO）。
#[inline]
pub fn install_kernel_satp_for_trap_handler() {
    let kernel_satp = KERNEL_TRAP_SATP.load(Ordering::Relaxed);
    paging::write_satp_and_flush(kernel_satp);
}

/// 根据即将返回的特权级（用户 / 内核）切换 `satp` 并刷新 TLB。
///
/// **注意**：本函数在 **仍为 S 态** 的 trap 返回路径上调用，随后还要继续执行 Rust / `trap.asm`
///（例如 `restore_current_trap_frame`、`ld`/`sret`）。因此 **不能** 假设「切到用户 `satp` 后内核虚拟地址立刻失效」。
///
/// 进入 handler 之前：`trap.asm` / `trap_entry_rust` 仍可能处于用户 `satp`，依赖用户表内
/// **`[0x8000_0000, phys_ram_end_exclusive)`** RAM 恒等映射（见 `wateros-mm` `kernel_elf`）。
/// handler 内来自用户的 trap 会先 [`install_kernel_satp_for_trap_handler`]，再执行 syscall 等；
/// `sret` 前由本函数装回用户 `satp`。
#[inline]
pub fn install_satp_for_exception_return(returns_to_user: bool) {
    let kernel_satp = KERNEL_TRAP_SATP.load(Ordering::Relaxed);
    let satp = if !returns_to_user {
        kernel_satp
    } else {
        let raw = scheduler::current_task_address_space_raw();
        if raw == 0 {
            kernel_satp
        } else {
            raw
        }
    };
    paging::write_satp_and_flush(satp);
}

/// 定时器中断路径：推进调度器 tick。
#[inline]
pub fn schedule_tick_from_trap() {
    crate::schedule_tick();
}

/// 将当前 trap 帧快照记入正在运行的任务（若存在）。
#[inline]
pub unsafe fn record_current_trap_frame(trap_frame_ptr: *const u8) {
    let trap_frame = unsafe { *(trap_frame_ptr as *const TaskTrapFrame) };
    scheduler::record_current_trap_frame(trap_frame);
}

/// 解析 trap 帧归属任务，返回应被 Rust 侧修改的权威 `TrapContext` 指针。
#[inline]
pub unsafe fn begin_current_trap_frame_access(trap_frame_ptr: *mut u8) -> *mut u8 {
    let trap_frame = unsafe { *(trap_frame_ptr as *const TaskTrapFrame) };
    scheduler::begin_current_trap_frame_access(trap_frame)
        .map(|p| p.cast::<u8>())
        .unwrap_or(trap_frame_ptr)
}

/// 将栈上的 trap 帧写回当前任务保存区。
#[inline]
pub unsafe fn restore_current_trap_frame(trap_frame_ptr: *mut u8) -> bool {
    let trap_frame = unsafe { &mut *(trap_frame_ptr as *mut TaskTrapFrame) };
    scheduler::restore_current_trap_frame(trap_frame)
}
