#![no_std]

use riscv::register::sstatus;

pub mod api {
    pub use ::api_v0::*;
}

pub mod scheduler {
    pub use ::scheduler::*;
}

#[cfg(feature = "impl-dummy")]
pub use impl_dummy as active_impl;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaitQueue {
    id: WaitQueueId,
}

impl WaitQueue {
    #[inline]
    pub fn new() -> Self { Self { id: scheduler::allocate_wait_queue() } }

    #[inline]
    pub const fn id(&self) -> WaitQueueId { self.id }

    #[inline]
    pub fn wait_current(&self) { scheduler::wait_current_on(self.id); }

    #[inline]
    pub fn wake_one(&self) -> Option<TaskId> { scheduler::wake_one_in_wait_queue(self.id) }

    #[inline]
    pub fn wake_all(&self) -> usize { scheduler::wake_all_in_wait_queue(self.id) }
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_schedule_tick() {
    schedule_tick();
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_record_current_trap_frame(trap_frame_ptr: *const u8) {
    let trap_frame = unsafe { *(trap_frame_ptr as *const TaskTrapFrame) };
    scheduler::record_current_trap_frame(trap_frame);
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_restore_current_trap_frame(trap_frame_ptr: *mut u8) -> bool {
    let trap_frame = unsafe { &mut *(trap_frame_ptr as *mut TaskTrapFrame) };
    scheduler::restore_current_trap_frame(trap_frame)
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_idle_task_main(_arg: usize) -> ! {
    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_idle_task_entry() -> ! {
    unsafe {
        sstatus::set_sie();
    }
    __wateros_idle_task_main(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_entry(task_start_ptr: usize) -> ! {
    let start = unsafe { &*(task_start_ptr as *const KernelTaskStart) };
    unsafe {
        sstatus::set_sie();
    }
    (start.entry)(start.arg)
}

pub use api_v0::{
    KernelTask, KernelTaskEntry, KernelTaskStart, ScheduleReason, TaskBlockReason, TaskExitCode,
    TaskId, TaskKind, TaskSnapshot, TaskState, TaskTick, TaskTrapFrame, WaitQueueId,
    IDLE_TASK_ID,
};

#[inline]
pub fn init() { scheduler::init(); }

#[inline]
pub fn spawn_kernel_task(entry: KernelTaskEntry, arg: usize) -> TaskId {
    scheduler::spawn_kernel_task(entry, arg)
}

#[inline]
pub fn run_first_task() -> ! { scheduler::run_first_task() }

#[inline]
pub fn yield_now() { scheduler::suspend_current_and_run_next(); }

#[inline]
pub fn schedule_tick() { scheduler::schedule_tick(); }

#[inline]
pub fn block_current(reason: TaskBlockReason) { scheduler::block_current(reason); }

#[inline]
pub fn sleep_for_ticks(ticks: TaskTick) { scheduler::sleep_current_for_ticks(ticks); }

#[inline]
pub fn wake_task(task_id: TaskId) -> bool { scheduler::wake_task(task_id) }

#[inline]
pub fn exit_current(exit_code: TaskExitCode) -> ! { scheduler::exit_current(exit_code) }

#[inline]
pub fn current_task_id() -> Option<TaskId> { scheduler::current_task_id() }

#[inline]
pub fn current_task_snapshot() -> Option<TaskSnapshot> { scheduler::current_task_snapshot() }
