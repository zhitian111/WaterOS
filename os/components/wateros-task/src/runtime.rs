//! 任务运行时：提供 **C ABI 符号**（供 arch 入口跳板）与 **Rust 具名入口**
//! （供组合层 trap handler 在进入/返回 trap 时访问当前任务现场）。
//!
//! ## C ABI 符号
//!
//! - [`__wateros_task_runtime_enter_current_user_task`]：
//!   用户任务首次被调度后恢复 trap 帧并返回用户态
//! - [`__wateros_idle_task_runtime_main`]：Idle 任务体
//! - [`__wateros_task_runtime_entry`]：普通内核任务 arch 入口
//!
//! ## Rust 入口（`pub(crate)`）
//!
//! - [`begin_current_trap_frame_access`]：进入 trap 时访问当前任务 trap 帧
//! - [`restore_current_trap_frame`]：返回前写回 trap 帧并准备地址空间 token
//!
//! 内核地址空间 token 由 [`mm_api::kernel_satp::get`] 读取（MM
//! 初始化后自动缓存到 `mm-api`）。

use crate::active_impl::TaskBootstrap;
use crate::scheduler;
use crate::scheduler::TaskTrapFrame;
use arch::trap::TrapAddressSpaceWrite;

// ============================================================================
// Rust 入口：地址空间 token 管理 & trap 帧访问
// ============================================================================

// 内核地址空间 token 由 `mm_api::kernel_satp` 模块缓存，此处直接读取。

fn return_address_space_token(returns_to_user : bool) -> usize {
    let kernel_token = mm_api::kernel_satp::get();
    if !returns_to_user {
        return kernel_token;
    }
    let raw = scheduler::current_task_address_space_raw();
    if raw == 0 {
        kernel_token
    } else {
        raw
    }
}

/// 当前任务返回用户态时应使用的地址空间 token。
#[inline]
fn current_user_return_address_space_token() -> usize { return_address_space_token(true) }

/// 解析 trap 帧归属任务，返回应被 Rust 侧修改的权威 `TrapContext` 指针。
#[inline]
pub(crate) unsafe fn begin_current_trap_frame_access(trap_frame_ptr : *mut u8) -> *mut u8 {
    let trap_frame = unsafe { *(trap_frame_ptr as *const TaskTrapFrame) };
    scheduler::begin_current_trap_frame_access(trap_frame).map(|p| p.cast::<u8>())
                                                          .unwrap_or(trap_frame_ptr)
}

/// 将当前任务保存区内的权威 trap 帧写回栈上 trap 帧，并写入返回地址空间 token。
#[inline]
pub(crate) unsafe fn restore_current_trap_frame(trap_frame_ptr : *mut u8) -> bool {
    let trap_frame = unsafe { &mut *(trap_frame_ptr as *mut TaskTrapFrame) };
    let restored = scheduler::restore_current_trap_frame(trap_frame);
    let token = return_address_space_token(
        <TaskTrapFrame as arch::trap::TrapFrameRead>::returns_to_user(trap_frame),
    );
    trap_frame.prepare_address_space_for_return(token);
    restored
}

// ============================================================================
// C ABI 符号：任务入口
// ============================================================================

unsafe extern "C" {
    /// 平台 arch：按 trap 帧与内核栈顶恢复用户态执行；trap
    /// 帧中必须已写入返回地址空间 token。
    fn __wateros_arch_restore_user_task(trap_frame_ptr : *const u8, kernel_stack_top : usize) -> !;
}

/// 用户任务首次被调度后的入口路径：恢复 trap 帧、写入用户地址空间 token 并跳到
/// arch 恢复例程。
#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_enter_current_user_task() -> ! {
    let mut trap_frame = TaskTrapFrame::default();
    let restored = scheduler::restore_current_trap_frame(&mut trap_frame);
    let kernel_stack_top = scheduler::current_task_kernel_stack_top().expect("user task entry \
                                                                              requires a current \
                                                                              task kernel stack");
    assert!(restored,
            "user task entry requires a prepared trap frame in the current task");
    trap_frame.set_return_address_space_token(current_user_return_address_space_token());
    unsafe {
        __wateros_arch_restore_user_task((&trap_frame as *const TaskTrapFrame).cast::<u8>(),
                                         kernel_stack_top)
    }
}

/// Idle 任务体：在内核态 `wfi` 等待中断。
///
/// **须**在首次进入时打开全局中断：[`schedule_tick`] 等路径在持有
/// [`InterruptGuard`] 时可能 `__switch` 到本任务，此时上一任务的 guard 尚未
/// `drop`，`SIE` 仍为关；若此处不 `enable`， `wfi` 在 QEMU/常见 RISC-V
/// 上可能等不到已挂起的 S 态定时器，表现为整机「卡死」在用户 `sret` 之后。
#[unsafe(no_mangle)]
pub extern "C" fn __wateros_idle_task_runtime_main(_arg : usize) -> ! {
    let _ = arch::interrupt::enable_global_interrupt();
    loop {
        arch::interrupt::wait_for_interrupt();
    }
}

/// 普通内核任务 arch 入口：`bootstrap_ptr` 指向
/// [`TaskBootstrap`]，启用中断后跳转到真实入口。
#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_entry(bootstrap_ptr : usize) -> ! {
    let bootstrap = unsafe { &*(bootstrap_ptr as *const TaskBootstrap) };
    arch::interrupt::enable_global_interrupt().expect("enable global interrupt for task runtime");
    bootstrap.run()
}
