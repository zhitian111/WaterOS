//! 与平台 trap / 用户态返回路径对接的运行时胶水：**C ABI 符号** 薄转发到 [`crate::trap_runtime`]，
//! 再委托 [`crate::scheduler`] 访问当前任务与 trap 现场。
//!
//! `#[no_mangle] extern "C"` 仍由汇编（如 `switch.S`）或固件约定直接按符号名链接；Rust 侧组合逻辑集中在 [`crate::trap_runtime`]。

use crate::active_impl::TaskBootstrap;
<<<<<<< HEAD
use crate::scheduler;
use crate::trap_runtime;
use riscv::register::sstatus;
=======
use crate::{schedule_tick, scheduler};
>>>>>>> github/main
use scheduler::TaskTrapFrame;

unsafe extern "C" {
    fn __wateros_arch_restore_user_task(trap_frame_ptr: *const u8, kernel_stack_top: usize) -> !;
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_install_trap_satp(returns_to_user: usize) {
    trap_runtime::install_satp_for_exception_return(returns_to_user != 0);
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_schedule_tick() {
    trap_runtime::schedule_tick_from_trap();
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_record_current_trap_frame(trap_frame_ptr: *const u8) {
    unsafe {
        trap_runtime::record_current_trap_frame(trap_frame_ptr);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_begin_current_trap_frame_access(
    trap_frame_ptr: *mut u8,
) -> *mut u8 {
    unsafe { trap_runtime::begin_current_trap_frame_access(trap_frame_ptr) }
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_restore_current_trap_frame(
    trap_frame_ptr: *mut u8,
) -> bool {
    unsafe { trap_runtime::restore_current_trap_frame(trap_frame_ptr) }
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_enter_current_user_task() -> ! {
    let mut trap_frame = TaskTrapFrame::default();
    let restored = scheduler::restore_current_trap_frame(&mut trap_frame);
    let kernel_stack_top = scheduler::current_task_kernel_stack_top()
        .expect("user task entry requires a current task kernel stack");
    assert!(
        restored,
        "user task entry requires a prepared trap frame in the current task"
    );
    unsafe {
        trap_runtime::install_satp_for_exception_return(true);
        __wateros_arch_restore_user_task(
            (&trap_frame as *const TaskTrapFrame).cast::<u8>(),
            kernel_stack_top,
        )
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_idle_task_runtime_main(_arg: usize) -> ! {
    loop {
        arch::interrupt::wait_for_interrupt();
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_entry(bootstrap_ptr: usize) -> ! {
    let bootstrap = unsafe { &*(bootstrap_ptr as *const TaskBootstrap) };
    arch::interrupt::enable_global_interrupt().expect("enable global interrupt for task runtime");
    bootstrap.run()
}
