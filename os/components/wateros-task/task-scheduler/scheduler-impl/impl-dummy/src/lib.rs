#![no_std]
#![allow(static_mut_refs)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use base::sync::UniprocessorSafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};
use riscv::register::sstatus;
use task_api::{
    KernelTaskEntry, ScheduleReason, TaskBlockReason, TaskContext, TaskExitCode, TaskId,
    TaskSnapshot, TaskTick, IDLE_TASK_ID,
};
use task_impl::TaskControlBlock;

unsafe extern "C" {
    fn __switch(current_task_cx_ptr: *mut TaskContext, next_task_cx_ptr: *const TaskContext);
    fn __task_entry();
}

type SwitchPair = (*mut TaskContext, *const TaskContext);

enum QueueTarget {
    Ready,
    Blocked(TaskBlockReason),
    Sleeping(TaskTick),
    Exited(TaskExitCode),
}

struct RoundRobinScheduler {
    bootstrap_task_cx: TaskContext,
    current: Option<Box<TaskControlBlock>>,
    ready_queue: VecDeque<Box<TaskControlBlock>>,
    blocked_queue: VecDeque<Box<TaskControlBlock>>,
    sleep_queue: VecDeque<Box<TaskControlBlock>>,
    exited_queue: VecDeque<Box<TaskControlBlock>>,
    idle_task: Option<Box<TaskControlBlock>>,
    next_task_id: TaskId,
    current_tick: TaskTick,
}

impl RoundRobinScheduler {
    fn new() -> Self {
        Self {
            bootstrap_task_cx: TaskContext::zero_init(),
            current: None,
            ready_queue: VecDeque::new(),
            blocked_queue: VecDeque::new(),
            sleep_queue: VecDeque::new(),
            exited_queue: VecDeque::new(),
            idle_task: None,
            next_task_id: IDLE_TASK_ID + 1,
            current_tick: 0,
        }
    }

    fn init(&mut self) {
        self.bootstrap_task_cx = TaskContext::zero_init();
        self.current = None;
        self.ready_queue.clear();
        self.blocked_queue.clear();
        self.sleep_queue.clear();
        self.exited_queue.clear();
        self.idle_task = Some(Box::new(TaskControlBlock::new_idle_task(
            IDLE_TASK_ID,
            __task_entry as usize,
            idle_task_main,
        )));
        self.next_task_id = IDLE_TASK_ID + 1;
        self.current_tick = 0;
    }

    fn spawn_kernel_task(&mut self, entry: KernelTaskEntry, arg: usize) -> TaskId {
        let task_id = self.next_task_id;
        self.next_task_id += 1;
        self.ready_queue.push_back(Box::new(TaskControlBlock::new_kernel_task(
            task_id,
            __task_entry as usize,
            entry,
            arg,
        )));
        log::debug!("[task-scheduler] spawned task {}", task_id);
        task_id
    }

    fn prepare_first_switch(&mut self) -> SwitchPair {
        self.promote_sleeping_tasks();
        let mut next = self.pick_next_task();
        next.mark_running();
        let current_task_cx_ptr = &mut self.bootstrap_task_cx as *mut TaskContext;
        let next_task_cx_ptr = next.context_ptr();
        self.current = Some(next);
        (current_task_cx_ptr, next_task_cx_ptr)
    }

    fn schedule(&mut self, reason: ScheduleReason) -> Option<SwitchPair> {
        match reason {
            ScheduleReason::Tick => {
                self.current_tick = self.current_tick.saturating_add(1);
                if let Some(current) = self.current.as_mut() {
                    if !current.is_idle() {
                        current.account_tick();
                    }
                }
            }
            ScheduleReason::Sleep(ticks) if ticks == 0 => {
                return self.schedule(ScheduleReason::Yield);
            }
            _ => {}
        }

        self.promote_sleeping_tasks();

        let mut current = self.current.take()?;
        let current_ptr = current.context_mut_ptr();
        let current_id = current.id();

        if current.is_idle() {
            return self.schedule_from_idle(current, current_ptr);
        }

        let queue_target = match reason {
            ScheduleReason::StartFirst => QueueTarget::Ready,
            ScheduleReason::Yield | ScheduleReason::Tick => QueueTarget::Ready,
            ScheduleReason::Block(block_reason) => QueueTarget::Blocked(block_reason),
            ScheduleReason::Sleep(ticks) => {
                let wake_tick = self.current_tick.saturating_add(ticks.max(1));
                QueueTarget::Sleeping(wake_tick)
            }
            ScheduleReason::Exit(exit_code) => QueueTarget::Exited(exit_code),
        };

        self.enqueue_task(current, queue_target);

        let mut next = self.pick_next_task();
        if next.id() == current_id {
            next.mark_running();
            self.current = Some(next);
            return None;
        }

        next.mark_running();
        let next_ptr = next.context_ptr();
        self.current = Some(next);
        Some((current_ptr, next_ptr))
    }

    fn schedule_from_idle(
        &mut self,
        mut idle_task: Box<TaskControlBlock>,
        current_ptr: *mut TaskContext,
    ) -> Option<SwitchPair> {
        let Some(mut next) = self.ready_queue.pop_front() else {
            idle_task.mark_running();
            self.current = Some(idle_task);
            return None;
        };
        next.mark_running();
        let next_ptr = next.context_ptr();
        self.idle_task = Some(idle_task);
        self.current = Some(next);
        Some((current_ptr, next_ptr))
    }

    fn enqueue_task(&mut self, mut task: Box<TaskControlBlock>, target: QueueTarget) {
        match target {
            QueueTarget::Ready => {
                task.mark_ready();
                self.ready_queue.push_back(task);
            }
            QueueTarget::Blocked(reason) => {
                task.mark_blocking(reason);
                self.blocked_queue.push_back(task);
            }
            QueueTarget::Sleeping(wake_tick) => {
                task.mark_sleeping(wake_tick);
                self.sleep_queue.push_back(task);
            }
            QueueTarget::Exited(exit_code) => {
                task.mark_exited(exit_code);
                self.exited_queue.push_back(task);
            }
        }
    }

    fn pick_next_task(&mut self) -> Box<TaskControlBlock> {
        if let Some(task) = self.ready_queue.pop_front() {
            task
        } else {
            self.idle_task
                .take()
                .expect("idle task must exist before scheduling")
        }
    }

    fn promote_sleeping_tasks(&mut self) {
        let mut still_sleeping = VecDeque::new();
        while let Some(mut task) = self.sleep_queue.pop_front() {
            if task.ready_to_wake(self.current_tick) {
                let task_id = task.id();
                task.mark_ready();
                self.ready_queue.push_back(task);
                log::trace!(
                    "[task-scheduler] wake sleeping task {} at tick {}",
                    task_id,
                    self.current_tick
                );
            } else {
                still_sleeping.push_back(task);
            }
        }
        self.sleep_queue = still_sleeping;
    }

    fn wake_task(&mut self, task_id: TaskId) -> bool {
        if let Some(mut task) = take_task_by_id(&mut self.blocked_queue, task_id) {
            task.mark_ready();
            self.ready_queue.push_back(task);
            return true;
        }
        if let Some(mut task) = take_task_by_id(&mut self.sleep_queue, task_id) {
            task.mark_ready();
            self.ready_queue.push_back(task);
            return true;
        }
        false
    }

    fn current_task_id(&self) -> Option<TaskId> { self.current.as_ref().map(|task| task.id()) }

    fn current_task_snapshot(&self) -> Option<TaskSnapshot> {
        self.current.as_ref().map(|task| task.snapshot())
    }
}

fn take_task_by_id(
    queue: &mut VecDeque<Box<TaskControlBlock>>,
    task_id: TaskId,
) -> Option<Box<TaskControlBlock>> {
    let mut remaining = VecDeque::new();
    let mut found = None;

    while let Some(task) = queue.pop_front() {
        if task.id() == task_id && found.is_none() {
            found = Some(task);
        } else {
            remaining.push_back(task);
        }
    }

    *queue = remaining;
    found
}

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

extern "C" fn idle_task_main(_arg: usize) -> ! {
    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_entry(entry_addr: usize, arg: usize) -> ! {
    let entry: KernelTaskEntry = unsafe { core::mem::transmute(entry_addr) };
    unsafe {
        sstatus::set_sie();
    }
    entry(arg)
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
