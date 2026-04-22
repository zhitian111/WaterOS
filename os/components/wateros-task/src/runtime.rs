use crate::active_impl::TaskBootstrap;
use crate::{schedule_tick, scheduler, TaskTrapFrame};
use riscv::register::sstatus;

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_schedule_tick() {
    schedule_tick();
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_record_current_trap_frame(trap_frame_ptr: *const u8) {
    let trap_frame = unsafe { *(trap_frame_ptr as *const TaskTrapFrame) };
    scheduler::record_current_trap_frame(trap_frame);
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_restore_current_trap_frame(
    trap_frame_ptr: *mut u8,
) -> bool {
    let trap_frame = unsafe { &mut *(trap_frame_ptr as *mut TaskTrapFrame) };
    scheduler::restore_current_trap_frame(trap_frame)
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
