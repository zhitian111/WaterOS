//! 运行时的 trap访问接口与任务入口
use crate::active_impl::TaskBootstrap;
use crate::scheduler;
use crate::scheduler::TaskTrapFrame;
// ============================================================================
// Rust 入口：地址空间 token 管理 & trap 帧访问
// ============================================================================

/// 解析 trap 帧归属任务，返回应被 Rust 侧修改的权威 `TrapContext` 指针。
pub(crate) unsafe fn begin_current_trap_frame_access(trap_frame_ptr : *mut u8) -> *mut u8 {
    let trap_frame = unsafe { *(trap_frame_ptr as *const TaskTrapFrame) };
    scheduler::begin_current_trap_frame_access(trap_frame).map(|p| p.cast::<u8>())
                                                          .unwrap_or(trap_frame_ptr)
}

/// 将当前任务保存区内的权威 trap 帧写回栈上 trap 帧，并写入返回地址空间 token。
pub(crate) unsafe fn restore_current_trap_frame(trap_frame_ptr : *mut u8) -> bool {
    let trap_frame = unsafe { &mut *(trap_frame_ptr as *mut TaskTrapFrame) };
    scheduler::restore_current_trap_frame(trap_frame)
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
    assert!(restored,
            "user task entry requires a prepared trap frame in the current task");
    let kernel_stack_top =
        scheduler::current_task_snapshot()
            .map(|snap| snap.kernel_stack_top)
            .expect("user task must have a kernel stack");
    unsafe {
        __wateros_arch_restore_user_task((&trap_frame as *const TaskTrapFrame).cast::<u8>(),
                                         kernel_stack_top)
    }
}

/// Idle任务入口：仅启用全局中断并循环等待中断。
#[unsafe(no_mangle)]
pub extern "C" fn __wateros_idle_task_runtime_main(_arg : usize) -> ! {
    let _ = arch::interrupt::enable_global_interrupt();
    loop {
        arch::interrupt::wait_for_interrupt();
    }
}

/// 普通内核任务入口
#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_entry(bootstrap_ptr : usize) -> ! {
    let bootstrap = unsafe { &*(bootstrap_ptr as *const TaskBootstrap) };
    arch::interrupt::enable_global_interrupt().expect("enable global interrupt for task runtime");
    bootstrap.run()
}
