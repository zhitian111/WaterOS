#![no_std]
#![allow(static_mut_refs)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use arch::task::ActiveArchTaskContext as TaskContext;
use base::sync::UniprocessorSafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};
use riscv::register::sstatus;
use task_api::{
    KernelTaskEntry, ScheduleReason, TaskBlockReason, TaskExitCode, TaskId, TaskSnapshot,
    TaskTick, TaskTrapFrame, WaitQueueId, IDLE_TASK_ID,
};
use task_impl::TaskControlBlock;

unsafe extern "C" {
    safe fn __wateros_idle_task_main(arg: usize) -> !;
    fn __switch(current_task_cx_ptr: *mut TaskContext, next_task_cx_ptr: *const TaskContext);
    fn __arch_idle_task_entry();
    fn __arch_task_entry();
}

type SwitchPair = (*mut TaskContext, *const TaskContext);

enum QueueTarget {
    Ready,
    Blocked(TaskBlockReason),
    Sleeping(TaskTick),
    Exited(TaskExitCode),
}

struct TaskTable {
    slots: Vec<Option<Box<TaskControlBlock>>>,
}

impl TaskTable {
    fn new() -> Self { Self { slots: Vec::new() } }

    fn clear(&mut self) { self.slots.clear(); }

    fn insert(&mut self, task: Box<TaskControlBlock>) {
        let task_id = task.id();
        if self.slots.len() <= task_id {
            self.slots.resize_with(task_id + 1, || None);
        }
        assert!(
            self.slots[task_id].is_none(),
            "task slot {} already occupied",
            task_id
        );
        self.slots[task_id] = Some(task);
    }

    fn task(&self, task_id: TaskId) -> &TaskControlBlock {
        self.slots
            .get(task_id)
            .and_then(|slot| slot.as_deref())
            .expect("task must exist in task table")
    }

    fn task_mut(&mut self, task_id: TaskId) -> &mut TaskControlBlock {
        self.slots
            .get_mut(task_id)
            .and_then(|slot| slot.as_deref_mut())
            .expect("task must exist in task table")
    }

    fn context_ptr(&self, task_id: TaskId) -> *const TaskContext { self.task(task_id).context_ptr() }

    fn context_mut_ptr(&mut self, task_id: TaskId) -> *mut TaskContext {
        self.task_mut(task_id).context_mut_ptr()
    }
}

struct RoundRobinScheduler {
    bootstrap_task_cx: TaskContext,
    task_table: TaskTable,
    current_task_id: Option<TaskId>,
    wait_queues: Vec<VecDeque<TaskId>>,
    ready_queue: VecDeque<TaskId>,
    blocked_queue: VecDeque<TaskId>,
    sleep_queue: VecDeque<TaskId>,
    exited_queue: VecDeque<TaskId>,
    next_task_id: TaskId,
    current_tick: TaskTick,
}

impl RoundRobinScheduler {
    fn new() -> Self {
        Self {
            bootstrap_task_cx: TaskContext::zero_init(),
            task_table: TaskTable::new(),
            current_task_id: None,
            wait_queues: Vec::new(),
            ready_queue: VecDeque::new(),
            blocked_queue: VecDeque::new(),
            sleep_queue: VecDeque::new(),
            exited_queue: VecDeque::new(),
            next_task_id: IDLE_TASK_ID + 1,
            current_tick: 0,
        }
    }

    fn init(&mut self) {
        self.bootstrap_task_cx = TaskContext::zero_init();
        self.task_table.clear();
        self.current_task_id = None;
        self.wait_queues.clear();
        self.ready_queue.clear();
        self.blocked_queue.clear();
        self.sleep_queue.clear();
        self.exited_queue.clear();
        self.task_table.insert(Box::new(TaskControlBlock::new_idle_task(
            IDLE_TASK_ID,
            __arch_idle_task_entry as usize,
            __wateros_idle_task_main,
        )));
        self.next_task_id = IDLE_TASK_ID + 1;
        self.current_tick = 0;
    }

    fn spawn_kernel_task(&mut self, entry: KernelTaskEntry, arg: usize) -> TaskId {
        let task_id = self.next_task_id;
        self.next_task_id += 1;
        self.task_table.insert(Box::new(TaskControlBlock::new_kernel_task(
            task_id,
            __arch_task_entry as usize,
            entry,
            arg,
        )));
        self.ready_queue.push_back(task_id);
        log::debug!("[task-scheduler] spawned task {}", task_id);
        task_id
    }

    fn allocate_wait_queue(&mut self) -> WaitQueueId {
        let wait_queue_id = self.wait_queues.len();
        self.wait_queues.push(VecDeque::new());
        wait_queue_id
    }

    fn prepare_first_switch(&mut self) -> SwitchPair {
        self.promote_sleeping_tasks();
        let current_task_cx_ptr = &mut self.bootstrap_task_cx as *mut TaskContext;
        let next_task_id = self.pick_next_task_id();
        self.task_table.task_mut(next_task_id).mark_running();
        let next_task_cx_ptr = self.task_table.context_ptr(next_task_id);
        self.current_task_id = Some(next_task_id);
        (current_task_cx_ptr, next_task_cx_ptr)
    }

    fn schedule(&mut self, reason: ScheduleReason) -> Option<SwitchPair> {
        match reason {
            ScheduleReason::Tick => {
                self.current_tick = self.current_tick.saturating_add(1);
                if let Some(current_task_id) = self.current_task_id {
                    if !self.task_table.task(current_task_id).is_idle() {
                        self.task_table.task_mut(current_task_id).account_tick();
                    }
                }
            }
            ScheduleReason::Sleep(ticks) if ticks == 0 => {
                return self.schedule(ScheduleReason::Yield);
            }
            _ => {}
        }

        self.promote_sleeping_tasks();

        let current_task_id = self.current_task_id.take()?;
        let current_ptr = self.task_table.context_mut_ptr(current_task_id);

        if self.task_table.task(current_task_id).is_idle() {
            return self.schedule_from_idle(current_task_id, current_ptr);
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

        self.enqueue_task(current_task_id, queue_target);

        let next_task_id = self.pick_next_task_id();
        if next_task_id == current_task_id {
            self.task_table.task_mut(next_task_id).mark_running();
            self.current_task_id = Some(next_task_id);
            return None;
        }

        self.task_table.task_mut(next_task_id).mark_running();
        let next_ptr = self.task_table.context_ptr(next_task_id);
        self.current_task_id = Some(next_task_id);
        Some((current_ptr, next_ptr))
    }

    fn schedule_from_idle(
        &mut self,
        idle_task_id: TaskId,
        current_ptr: *mut TaskContext,
    ) -> Option<SwitchPair> {
        let Some(next_task_id) = self.ready_queue.pop_front() else {
            self.task_table.task_mut(idle_task_id).mark_running();
            self.current_task_id = Some(idle_task_id);
            return None;
        };
        self.task_table.task_mut(next_task_id).mark_running();
        let next_ptr = self.task_table.context_ptr(next_task_id);
        self.current_task_id = Some(next_task_id);
        Some((current_ptr, next_ptr))
    }

    fn enqueue_task(&mut self, task_id: TaskId, target: QueueTarget) {
        match target {
            QueueTarget::Ready => {
                self.task_table.task_mut(task_id).mark_ready();
                self.ready_queue.push_back(task_id);
            }
            QueueTarget::Blocked(reason) => {
                self.task_table.task_mut(task_id).mark_blocking(reason);
                match reason {
                    TaskBlockReason::WaitQueue(wait_queue_id) => {
                        self.wait_queue_mut(wait_queue_id).push_back(task_id);
                    }
                    _ => self.blocked_queue.push_back(task_id),
                }
            }
            QueueTarget::Sleeping(wake_tick) => {
                self.task_table.task_mut(task_id).mark_sleeping(wake_tick);
                self.sleep_queue.push_back(task_id);
            }
            QueueTarget::Exited(exit_code) => {
                self.task_table.task_mut(task_id).mark_exited(exit_code);
                self.exited_queue.push_back(task_id);
            }
        }
    }

    fn pick_next_task_id(&mut self) -> TaskId { self.ready_queue.pop_front().unwrap_or(IDLE_TASK_ID) }

    fn wait_queue_mut(&mut self, wait_queue_id: WaitQueueId) -> &mut VecDeque<TaskId> {
        self.wait_queues
            .get_mut(wait_queue_id)
            .expect("wait queue must exist before use")
    }

    fn promote_sleeping_tasks(&mut self) {
        let mut still_sleeping = VecDeque::new();
        while let Some(task_id) = self.sleep_queue.pop_front() {
            if self.task_table.task(task_id).ready_to_wake(self.current_tick) {
                self.task_table.task_mut(task_id).mark_ready();
                self.ready_queue.push_back(task_id);
                log::trace!(
                    "[task-scheduler] wake sleeping task {} at tick {}",
                    task_id,
                    self.current_tick
                );
            } else {
                still_sleeping.push_back(task_id);
            }
        }
        self.sleep_queue = still_sleeping;
    }

    fn wake_task(&mut self, task_id: TaskId) -> bool {
        if take_task_id_by_id(&mut self.blocked_queue, task_id) {
            self.task_table.task_mut(task_id).mark_ready();
            self.ready_queue.push_back(task_id);
            return true;
        }
        if take_task_id_by_id(&mut self.sleep_queue, task_id) {
            self.task_table.task_mut(task_id).mark_ready();
            self.ready_queue.push_back(task_id);
            return true;
        }
        for wait_queue in &mut self.wait_queues {
            if take_task_id_by_id(wait_queue, task_id) {
                self.task_table.task_mut(task_id).mark_ready();
                self.ready_queue.push_back(task_id);
                return true;
            }
        }
        false
    }

    fn wake_one_in_wait_queue(&mut self, wait_queue_id: WaitQueueId) -> Option<TaskId> {
        let task_id = self.wait_queue_mut(wait_queue_id).pop_front()?;
        self.task_table.task_mut(task_id).mark_ready();
        self.ready_queue.push_back(task_id);
        Some(task_id)
    }

    fn wake_all_in_wait_queue(&mut self, wait_queue_id: WaitQueueId) -> usize {
        let mut woken = 0usize;
        while let Some(task_id) = self.wait_queue_mut(wait_queue_id).pop_front() {
            self.task_table.task_mut(task_id).mark_ready();
            self.ready_queue.push_back(task_id);
            woken = woken.saturating_add(1);
        }
        woken
    }

    fn current_task_id(&self) -> Option<TaskId> { self.current_task_id }

    fn current_task_snapshot(&self) -> Option<TaskSnapshot> {
        self.current_task_id
            .map(|task_id| self.task_table.task(task_id).snapshot())
    }

    fn record_current_trap_frame(&mut self, trap_frame: TaskTrapFrame) {
        if let Some(current_task_id) = self.current_task_id {
            self.task_table
                .task_mut(current_task_id)
                .record_trap_frame(trap_frame);
        }
    }

    fn restore_current_trap_frame(&self, trap_frame: &mut TaskTrapFrame) -> bool {
        self.current_task_id
            .map(|current_task_id| self.task_table.task(current_task_id).restore_trap_frame_into(trap_frame))
            .unwrap_or(false)
    }
}

fn take_task_id_by_id(queue: &mut VecDeque<TaskId>, task_id: TaskId) -> bool {
    let mut remaining = VecDeque::new();
    let mut found = false;

    while let Some(candidate_task_id) = queue.pop_front() {
        if candidate_task_id == task_id && !found {
            found = true;
        } else {
            remaining.push_back(candidate_task_id);
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

pub fn wait_current_on(wait_queue_id: WaitQueueId) {
    let _guard = InterruptGuard::new();
    let switch_pair =
        with_scheduler(|scheduler| scheduler.schedule(ScheduleReason::Block(TaskBlockReason::WaitQueue(wait_queue_id))));
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

pub fn record_current_trap_frame(trap_frame: TaskTrapFrame) {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.record_current_trap_frame(trap_frame));
}

pub fn restore_current_trap_frame(trap_frame: &mut TaskTrapFrame) -> bool {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.restore_current_trap_frame(trap_frame))
}
