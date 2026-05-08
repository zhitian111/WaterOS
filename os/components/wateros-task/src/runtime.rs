//! 与平台 trap / 用户态返回路径对接的运行时胶水：**C ABI 符号** 薄转发到
//! [`crate::trap_runtime`]， 再委托 [`crate::scheduler`] 访问当前任务与 trap
//! 现场。
//!
//! `#[no_mangle] extern "C"` 仍由汇编（如
//! `switch.S`）或固件约定直接按符号名链接；Rust 侧组合逻辑集中在
//! [`crate::trap_runtime`]。

use crate::active_impl::TaskBootstrap;
use crate::scheduler;
use crate::trap_runtime;
// use riscv::register::sstatus;

use scheduler::TaskTrapFrame;

unsafe extern "C" {
    /// 平台 arch：按 trap 帧与内核栈顶恢复用户态执行；由 `enter_current_user_task` 在 `satp` 就绪后调用。
    fn __wateros_arch_restore_user_task(trap_frame_ptr : *const u8, kernel_stack_top : usize) -> !;
}

/// Trap 返回路径：按是否回到用户态安装 `satp` 并刷新 TLB（C 符号，供汇编/链接按名调用）。
#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_install_trap_satp(returns_to_user : usize) {
    trap_runtime::install_satp_for_exception_return(returns_to_user != 0);
}

/// 定时器 trap 入口：将 tick 委托给 [`trap_runtime::schedule_tick_from_trap`]。
#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_schedule_tick() {
    trap_runtime::schedule_tick_from_trap();
}

/// 将当前 CPU 上的 trap 帧快照写入当前任务的 TCB（通常在进入 Rust trap 处理早期调用）。
#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_record_current_trap_frame(trap_frame_ptr : *const u8) {
    unsafe {
        trap_runtime::record_current_trap_frame(trap_frame_ptr);
    }
}

/// 解析 trap 帧归属并返回应由 Rust 修改的权威 trap 上下文指针（可能仍指向调用方缓冲区）。
#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_begin_current_trap_frame_access(trap_frame_ptr: *mut u8)
                                                                         -> *mut u8 {
    unsafe { trap_runtime::begin_current_trap_frame_access(trap_frame_ptr) }
}

/// 将栈上 trap 缓冲区写回当前任务保存区；返回是否成功匹配到已保存现场。
#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_restore_current_trap_frame(trap_frame_ptr : *mut u8)
                                                                    -> bool {
    unsafe { trap_runtime::restore_current_trap_frame(trap_frame_ptr) }
}

/// 用户任务首次下陷后进入用户态的路径：恢复 trap 帧、安装用户 `satp` 并跳到 arch 恢复例程。
#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_enter_current_user_task() -> ! {
    let mut trap_frame = TaskTrapFrame::default();
    let restored = scheduler::restore_current_trap_frame(&mut trap_frame);
    let kernel_stack_top = scheduler::current_task_kernel_stack_top().expect("user task entry \
                                                                              requires a current \
                                                                              task kernel stack");
    assert!(restored,
            "user task entry requires a prepared trap frame in the current task");
    unsafe {
        trap_runtime::install_satp_for_exception_return(true);
        __wateros_arch_restore_user_task((&trap_frame as *const TaskTrapFrame).cast::<u8>(),
                                         kernel_stack_top)
    }
}

/// Idle 任务体：关闭抢占时在内核态自旋等待中断（与 `IDLE_TASK_ID` 绑定）。
#[unsafe(no_mangle)]
pub extern "C" fn __wateros_idle_task_runtime_main(_arg : usize) -> ! {
    loop {
        arch::interrupt::wait_for_interrupt();
    }
}

/// 普通内核任务 arch 入口：`bootstrap_ptr` 指向 [`TaskBootstrap`]，启用中断后跳转到真实入口。
#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_entry(bootstrap_ptr : usize) -> ! {
    let bootstrap = unsafe { &*(bootstrap_ptr as *const TaskBootstrap) };
    arch::interrupt::enable_global_interrupt().expect("enable global interrupt for task runtime");
    bootstrap.run()
}
