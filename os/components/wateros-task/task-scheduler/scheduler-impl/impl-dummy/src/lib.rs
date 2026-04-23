#![no_std]
#![allow(static_mut_refs)]

use arch::task::ActiveArchTaskContext as TaskContext;
use base::sync::UniprocessorSafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};
use riscv::register::sstatus;
use task_api::{
    ExitedTask, KernelTaskEntry, ScheduleReason, TaskBlockReason, TaskExitCode, TaskId,
    TaskSnapshot, TaskTick, TaskTrapFrame, TaskWaitHandle, TaskWaitResult, UserTaskEntryPc,
    WaitQueueId,
};

mod queues;
mod registry;
mod scheduler;

use scheduler::RoundRobinScheduler;

unsafe extern "C" {
    fn __switch(current_task_cx_ptr: *mut TaskContext, next_task_cx_ptr: *const TaskContext);
}

type SwitchPair = (*mut TaskContext, *const TaskContext);

static mut SCHEDULER: MaybeUninit<UniprocessorSafeCell<RoundRobinScheduler>> = MaybeUninit::uninit();
static SCHEDULER_READY: AtomicBool = AtomicBool::new(false);

fn scheduler_cell() -> &'static UniprocessorSafeCell<RoundRobinScheduler> {
    assert!(
        SCHEDULER_READY.load(Ordering::Acquire),
        "scheduler not initialized: call init_scheduler() first"
    );
    unsafe { &*SCHEDULER.as_ptr() }
}

fn with_scheduler<R>(f: impl FnOnce(&mut RoundRobinScheduler) -> R) -> R {
    let mut scheduler = scheduler_cell().exclusive_access();
    f(&mut scheduler)
}

struct InterruptGuard {
    restore_sie: bool,
}

impl InterruptGuard {
    fn new() -> Self {
        let restore_sie = sstatus::read().sie();
        unsafe {
            sstatus::clear_sie();
        }
        Self { restore_sie }
    }
}

impl Drop for InterruptGuard {
    fn drop(&mut self) {
        if self.restore_sie {
            unsafe {
                sstatus::set_sie();
            }
        }
    }
}

pub fn init_scheduler() {
    if !SCHEDULER_READY.load(Ordering::Acquire) {
        unsafe {
            SCHEDULER.write(UniprocessorSafeCell::new(RoundRobinScheduler::new()));
        }
        SCHEDULER_READY.store(true, Ordering::Release);
    }
    with_scheduler(|scheduler| scheduler.init());
    log::info!("[task-scheduler] initialized");
}

pub fn spawn_kernel_task(entry: KernelTaskEntry, arg: usize) -> TaskId {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.spawn_kernel_task(entry, arg))
}

pub fn spawn_user_task(entry_pc: UserTaskEntryPc) -> TaskId {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.spawn_user_task(entry_pc))
}

pub fn allocate_wait_queue() -> WaitQueueId {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.allocate_wait_queue())
}

pub fn run_first_task() -> ! {
    let (current_task_cx_ptr, next_task_cx_ptr) =
        with_scheduler(|scheduler| scheduler.prepare_first_switch());
    unsafe {
        __switch(current_task_cx_ptr, next_task_cx_ptr);
    }
    panic!("run_first_task must not return");
}

pub fn suspend_current_and_run_next() {
    let _guard = InterruptGuard::new();
    let switch_pair = with_scheduler(|scheduler| scheduler.schedule(ScheduleReason::Yield));
    if let Some((current_task_cx_ptr, next_task_cx_ptr)) = switch_pair {
        unsafe {
            __switch(current_task_cx_ptr, next_task_cx_ptr);
        }
    }
}

pub fn schedule_tick() {
    let _guard = InterruptGuard::new();
    let switch_pair = with_scheduler(|scheduler| scheduler.schedule(ScheduleReason::Tick));
    if let Some((current_task_cx_ptr, next_task_cx_ptr)) = switch_pair {
        unsafe {
            __switch(current_task_cx_ptr, next_task_cx_ptr);
        }
    }
}

pub fn block_current(reason: TaskBlockReason) {
    let _guard = InterruptGuard::new();
    let switch_pair = with_scheduler(|scheduler| scheduler.schedule(ScheduleReason::Block(reason)));
    if let Some((current_task_cx_ptr, next_task_cx_ptr)) = switch_pair {
        unsafe {
            __switch(current_task_cx_ptr, next_task_cx_ptr);
        }
    }
}

pub fn wait_current(wait_handle: TaskWaitHandle) {
    let _guard = InterruptGuard::new();
    let switch_pair = with_scheduler(|scheduler| scheduler.schedule_wait(wait_handle, None));
    if let Some((current_task_cx_ptr, next_task_cx_ptr)) = switch_pair {
        unsafe {
            __switch(current_task_cx_ptr, next_task_cx_ptr);
        }
    }
}

pub fn wait_current_timeout(wait_handle: TaskWaitHandle, timeout_ticks: TaskTick) -> TaskWaitResult {
    if timeout_ticks == 0 {
        return TaskWaitResult::TimedOut;
    }

    let _guard = InterruptGuard::new();
    let switch_pair =
        with_scheduler(|scheduler| scheduler.schedule_wait(wait_handle, Some(timeout_ticks)));
    if let Some((current_task_cx_ptr, next_task_cx_ptr)) = switch_pair {
        unsafe {
            __switch(current_task_cx_ptr, next_task_cx_ptr);
        }
    }
    with_scheduler(|scheduler| scheduler.take_current_wait_result())
}

pub fn wait_current_on(wait_queue_id: WaitQueueId) {
    wait_current(TaskWaitHandle::for_wait_queue(wait_queue_id));
}

pub fn wait_current_on_timeout(wait_queue_id: WaitQueueId, timeout_ticks: TaskTick) -> TaskWaitResult {
    wait_current_timeout(TaskWaitHandle::for_wait_queue(wait_queue_id), timeout_ticks)
}

pub fn wait_for_task_exit(task_id: TaskId) {
    wait_current(TaskWaitHandle::for_task_exit(task_id));
}

pub fn wait_for_task_exit_timeout(task_id: TaskId, timeout_ticks: TaskTick) -> TaskWaitResult {
    wait_current_timeout(TaskWaitHandle::for_task_exit(task_id), timeout_ticks)
}

pub fn sleep_current_for_ticks(ticks: TaskTick) {
    let _guard = InterruptGuard::new();
    let switch_pair = with_scheduler(|scheduler| scheduler.schedule(ScheduleReason::Sleep(ticks)));
    if let Some((current_task_cx_ptr, next_task_cx_ptr)) = switch_pair {
        unsafe {
            __switch(current_task_cx_ptr, next_task_cx_ptr);
        }
    }
}

pub fn wake_task(task_id: TaskId) -> bool {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.wake_task(task_id))
}

pub fn reap_exited_task(task_id: TaskId) -> Option<ExitedTask> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.reap_exited_task(task_id))
}

pub fn reap_one_exited_task() -> Option<ExitedTask> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.reap_one_exited_task())
}

pub fn wake_one_in_wait_queue(wait_queue_id: WaitQueueId) -> Option<TaskId> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.wake_one_in_wait_queue(wait_queue_id))
}

pub fn wake_all_in_wait_queue(wait_queue_id: WaitQueueId) -> usize {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.wake_all_in_wait_queue(wait_queue_id))
}

pub fn exit_current(exit_code: TaskExitCode) -> ! {
    let _guard = InterruptGuard::new();
    let switch_pair = with_scheduler(|scheduler| scheduler.schedule(ScheduleReason::Exit(exit_code)));
    if let Some((current_task_cx_ptr, next_task_cx_ptr)) = switch_pair {
        unsafe {
            __switch(current_task_cx_ptr, next_task_cx_ptr);
        }
    }
    panic!("exit_current must not resume the exited task");
}

pub fn current_task_id() -> Option<TaskId> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.current_task_id())
}

pub fn current_task_snapshot() -> Option<TaskSnapshot> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.current_task_snapshot())
}

pub fn current_task_kernel_stack_top() -> Option<usize> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.current_task_kernel_stack_top())
}

pub fn record_current_trap_frame(trap_frame: TaskTrapFrame) {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.record_current_trap_frame(trap_frame));
}

pub fn begin_current_trap_frame_access(trap_frame: TaskTrapFrame) -> Option<*mut TaskTrapFrame> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.begin_current_trap_frame_access(trap_frame))
}

pub fn restore_current_trap_frame(trap_frame: &mut TaskTrapFrame) -> bool {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.restore_current_trap_frame(trap_frame))
}
