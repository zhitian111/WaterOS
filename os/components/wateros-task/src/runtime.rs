use crate::active_impl::TaskBootstrap;
use crate::{schedule_tick, scheduler, TaskTrapFrame};
use riscv::register::sstatus;

unsafe extern "C" {
    fn __wateros_arch_restore_user_task(trap_frame_ptr: *const u8) -> !;
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_schedule_tick() {
    schedule_tick();
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_yield_current() {
    crate::yield_now();
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_exit_current(exit_code: isize) -> ! {
    crate::exit_current(exit_code)
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_record_current_trap_frame(trap_frame_ptr: *const u8) {
    let trap_frame = unsafe { *(trap_frame_ptr as *const TaskTrapFrame) };
    scheduler::record_current_trap_frame(trap_frame);
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_begin_current_trap_frame_access(
    trap_frame_ptr: *mut u8,
) -> *mut u8 {
    let trap_frame = unsafe { *(trap_frame_ptr as *const TaskTrapFrame) };
    scheduler::begin_current_trap_frame_access(trap_frame)
        .map(|trap_frame_ptr| trap_frame_ptr.cast::<u8>())
        .unwrap_or(trap_frame_ptr)
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_restore_current_trap_frame(
    trap_frame_ptr: *mut u8,
) -> bool {
    let trap_frame = unsafe { &mut *(trap_frame_ptr as *mut TaskTrapFrame) };
    scheduler::restore_current_trap_frame(trap_frame)
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_enter_current_user_task() -> ! {
    let mut trap_frame = TaskTrapFrame::default();
    let restored = scheduler::restore_current_trap_frame(&mut trap_frame);
    assert!(
        restored,
        "user task entry requires a prepared trap frame in the current task"
    );
    unsafe { __wateros_arch_restore_user_task((&trap_frame as *const TaskTrapFrame).cast::<u8>()) }
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_idle_task_runtime_main(_arg: usize) -> ! {
    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_idle_task_runtime_entry() -> ! {
    unsafe {
        sstatus::set_sie();
    }
    __wateros_idle_task_runtime_main(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_entry(bootstrap_ptr: usize) -> ! {
    let bootstrap = unsafe { &*(bootstrap_ptr as *const TaskBootstrap) };
    unsafe {
        sstatus::set_sie();
    }
    bootstrap.run()
}
