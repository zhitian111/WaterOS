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
    ExitedTask, KernelTaskEntry, ScheduleReason, TaskBlockReason, TaskExitCode, TaskId,
    TaskSnapshot, TaskState, TaskTick, TaskTrapFrame, TaskWaitHandle, TaskWaitResult,
    TaskWaitTarget, UserTaskEntryPc, WaitQueueId, IDLE_TASK_ID,
};
use task_impl::TaskControlBlock;

unsafe extern "C" {
    safe fn __wateros_idle_task_runtime_main(arg: usize) -> !;
    fn __switch(current_task_cx_ptr: *mut TaskContext, next_task_cx_ptr: *const TaskContext);
    fn __arch_idle_task_entry();
    fn __arch_task_entry();
    fn __arch_user_task_entry();
}

type SwitchPair = (*mut TaskContext, *const TaskContext);

enum QueueTarget {
    Ready,
    Blocked(TaskBlockReason),
    Sleeping(TaskTick),
    Exited(TaskExitCode),
}

#[derive(Clone, Copy)]
struct WaitTimeoutEntry {
    task_id: TaskId,
    wait_handle: TaskWaitHandle,
    wake_tick: TaskTick,
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

    fn remove(&mut self, task_id: TaskId) -> Option<Box<TaskControlBlock>> {
        self.slots.get_mut(task_id).and_then(|slot| slot.take())
    }
}

struct TaskRegistry {
    bootstrap_task_cx: TaskContext,
    task_table: TaskTable,
    current_task_id: Option<TaskId>,
    next_task_id: TaskId,
}

impl TaskRegistry {
    fn new() -> Self {
        Self {
            bootstrap_task_cx: TaskContext::zero_init(),
            task_table: TaskTable::new(),
            current_task_id: None,
            next_task_id: IDLE_TASK_ID + 1,
        }
    }

    fn init(&mut self) {
        self.bootstrap_task_cx = TaskContext::zero_init();
        self.task_table.clear();
        self.current_task_id = None;
        self.task_table.insert(Box::new(TaskControlBlock::new_idle_task(
            IDLE_TASK_ID,
            __arch_idle_task_entry as usize,
            __wateros_idle_task_runtime_main,
        )));
        self.next_task_id = IDLE_TASK_ID + 1;
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
        task_id
    }

    fn spawn_user_task(&mut self, entry_pc: UserTaskEntryPc) -> TaskId {
        let task_id = self.next_task_id;
        self.next_task_id += 1;
        self.task_table.insert(Box::new(TaskControlBlock::new_user_task(
            task_id,
            __arch_user_task_entry as usize,
            entry_pc,
        )));
        task_id
    }

    fn first_switch_to(&mut self, next_task_id: TaskId) -> SwitchPair {
        let current_task_cx_ptr = &mut self.bootstrap_task_cx as *mut TaskContext;
        let next_task_cx_ptr = self.mark_running_and_set_current(next_task_id);
        (current_task_cx_ptr, next_task_cx_ptr)
    }

    fn take_current_switch_out(&mut self) -> Option<(TaskId, *mut TaskContext)> {
        let current_task_id = self.current_task_id.take()?;
        let current_ptr = self.task_table.task_mut(current_task_id).context_mut_ptr();
        Some((current_task_id, current_ptr))
    }

    fn mark_running_and_set_current(&mut self, task_id: TaskId) -> *const TaskContext {
        self.task_table.task_mut(task_id).mark_running();
        self.current_task_id = Some(task_id);
        self.task_table.task(task_id).context_ptr()
    }

    fn mark_ready(&mut self, task_id: TaskId) { self.task_table.task_mut(task_id).mark_ready(); }

    fn mark_blocking(&mut self, task_id: TaskId, reason: TaskBlockReason) {
        self.task_table.task_mut(task_id).mark_blocking(reason);
    }

    fn mark_sleeping(&mut self, task_id: TaskId, wake_tick: TaskTick) {
        self.task_table.task_mut(task_id).mark_sleeping(wake_tick);
    }

    fn mark_exited(&mut self, task_id: TaskId, exit_code: TaskExitCode) {
        self.task_table.task_mut(task_id).mark_exited(exit_code);
    }

    fn ready_to_wake(&self, task_id: TaskId, current_tick: TaskTick) -> bool {
        self.task_table.task(task_id).ready_to_wake(current_tick)
    }

    fn is_idle(&self, task_id: TaskId) -> bool { self.task_table.task(task_id).is_idle() }

    fn state(&self, task_id: TaskId) -> Option<TaskState> {
        self.task_table
            .slots
            .get(task_id)
            .and_then(|slot| slot.as_deref())
            .map(TaskControlBlock::state)
    }

    fn wait_target_ready(&self, wait_handle: TaskWaitHandle) -> bool {
        match wait_handle.target() {
            TaskWaitTarget::WaitQueue(_) => false,
            TaskWaitTarget::TaskExit(task_id) => self
                .state(task_id)
                .map(|state| matches!(state, TaskState::Exited(_)))
                .unwrap_or(true),
        }
    }

    fn account_tick_for_current(&mut self) {
        if let Some(current_task_id) = self.current_task_id {
            if !self.task_table.task(current_task_id).is_idle() {
                self.task_table.task_mut(current_task_id).account_tick();
            }
        }
    }

    fn current_task_id(&self) -> Option<TaskId> { self.current_task_id }

    fn current_task_snapshot(&self) -> Option<TaskSnapshot> {
        self.current_task_id
            .map(|task_id| self.task_table.task(task_id).snapshot())
    }

    fn current_task_kernel_stack_top(&self) -> Option<usize> {
        self.current_task_id
            .map(|task_id| self.task_table.task(task_id).kernel_stack_top())
    }

    fn clear_wait_result(&mut self, task_id: TaskId) {
        self.task_table.task_mut(task_id).clear_wait_result();
    }

    fn finish_wait(&mut self, task_id: TaskId, result: TaskWaitResult) {
        self.task_table.task_mut(task_id).finish_wait(result);
    }

    fn take_current_wait_result(&mut self) -> TaskWaitResult {
        let current_task_id = self
            .current_task_id
            .expect("wait result can only be taken for a running task");
        self.task_table.task_mut(current_task_id).take_wait_result()
    }

    fn reap_task(&mut self, task_id: TaskId) -> Option<ExitedTask> {
        let task = self.task_table.remove(task_id)?;
        task.exited_task()
    }

    fn record_current_trap_frame(&mut self, trap_frame: TaskTrapFrame) {
        if let Some(current_task_id) = self.current_task_id {
            self.task_table
                .task_mut(current_task_id)
                .record_trap_frame(trap_frame);
        }
    }

    fn begin_current_trap_frame_access(
        &mut self,
        trap_frame: TaskTrapFrame,
    ) -> Option<*mut TaskTrapFrame> {
        let current_task_id = self.current_task_id?;
        Some(
            self.task_table
                .task_mut(current_task_id)
                .begin_trap_frame_access(trap_frame),
        )
    }

    fn restore_current_trap_frame(&self, trap_frame: &mut TaskTrapFrame) -> bool {
        self.current_task_id
            .map(|current_task_id| {
                self.task_table
                    .task(current_task_id)
                    .restore_trap_frame_into(trap_frame)
            })
            .unwrap_or(false)
    }
}

struct RoundRobinQueues {
    wait_queues: Vec<VecDeque<TaskId>>,
    exit_wait_queues: Vec<VecDeque<TaskId>>,
    wait_timeouts: VecDeque<WaitTimeoutEntry>,
    ready_queue: VecDeque<TaskId>,
    blocked_queue: VecDeque<TaskId>,
    sleep_queue: VecDeque<TaskId>,
    exited_queue: VecDeque<TaskId>,
    current_tick: TaskTick,
}

impl RoundRobinQueues {
    fn new() -> Self {
        Self {
            wait_queues: Vec::new(),
            exit_wait_queues: Vec::new(),
            wait_timeouts: VecDeque::new(),
            ready_queue: VecDeque::new(),
            blocked_queue: VecDeque::new(),
            sleep_queue: VecDeque::new(),
            exited_queue: VecDeque::new(),
            current_tick: 0,
        }
    }

    fn init(&mut self) {
        self.wait_queues.clear();
        self.exit_wait_queues.clear();
        self.wait_timeouts.clear();
        self.ready_queue.clear();
        self.blocked_queue.clear();
        self.sleep_queue.clear();
        self.exited_queue.clear();
        self.current_tick = 0;
    }

    fn allocate_wait_queue(&mut self) -> WaitQueueId {
        let wait_queue_id = self.wait_queues.len();
        self.wait_queues.push(VecDeque::new());
        wait_queue_id
    }

    fn push_spawned_task(&mut self, task_id: TaskId) { self.ready_queue.push_back(task_id); }

    fn on_tick(&mut self) { self.current_tick = self.current_tick.saturating_add(1); }

    fn current_tick(&self) -> TaskTick { self.current_tick }

    fn pick_next_task_id(&mut self) -> TaskId { self.ready_queue.pop_front().unwrap_or(IDLE_TASK_ID) }

    fn enqueue_task(&mut self, registry: &mut TaskRegistry, task_id: TaskId, target: QueueTarget) {
        match target {
            QueueTarget::Ready => {
                registry.mark_ready(task_id);
                self.ready_queue.push_back(task_id);
            }
            QueueTarget::Blocked(reason) => {
                registry.mark_blocking(task_id, reason);
                match reason {
                    TaskBlockReason::Wait(wait_handle) => {
                        self.wait_list_mut(wait_handle).push_back(task_id);
                    }
                    _ => self.blocked_queue.push_back(task_id),
                }
            }
            QueueTarget::Sleeping(wake_tick) => {
                registry.mark_sleeping(task_id, wake_tick);
                self.sleep_queue.push_back(task_id);
            }
            QueueTarget::Exited(exit_code) => {
                registry.mark_exited(task_id, exit_code);
                self.exited_queue.push_back(task_id);
                self.wake_all_waiters_for_task_exit(registry, task_id);
            }
        }
    }

    fn enqueue_wait_timeout(&mut self, task_id: TaskId, wait_handle: TaskWaitHandle, wake_tick: TaskTick) {
        self.wait_timeouts.push_back(WaitTimeoutEntry {
            task_id,
            wait_handle,
            wake_tick,
        });
    }

    fn promote_sleeping_tasks(&mut self, registry: &mut TaskRegistry) {
        let mut still_sleeping = VecDeque::new();
        while let Some(task_id) = self.sleep_queue.pop_front() {
            if registry.ready_to_wake(task_id, self.current_tick) {
                registry.mark_ready(task_id);
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

    fn promote_wait_timeouts(&mut self, registry: &mut TaskRegistry) {
        let mut pending = VecDeque::new();
        while let Some(entry) = self.wait_timeouts.pop_front() {
            if entry.wake_tick > self.current_tick {
                pending.push_back(entry);
                continue;
            }

            let still_waiting = matches!(
                registry.state(entry.task_id),
                Some(TaskState::Blocking(TaskBlockReason::Wait(wait_handle)))
                    if wait_handle == entry.wait_handle
            );
            if !still_waiting {
                continue;
            }

            if self.remove_from_wait_list(entry.wait_handle, entry.task_id) {
                registry.finish_wait(entry.task_id, TaskWaitResult::TimedOut);
                registry.mark_ready(entry.task_id);
                self.ready_queue.push_back(entry.task_id);
            }
        }
        self.wait_timeouts = pending;
    }

    fn wake_task(&mut self, registry: &mut TaskRegistry, task_id: TaskId) -> bool {
        if take_task_id_by_id(&mut self.blocked_queue, task_id) {
            registry.finish_wait(task_id, TaskWaitResult::Woken);
            registry.mark_ready(task_id);
            self.ready_queue.push_back(task_id);
            return true;
        }
        if take_task_id_by_id(&mut self.sleep_queue, task_id) {
            registry.finish_wait(task_id, TaskWaitResult::Woken);
            registry.mark_ready(task_id);
            self.ready_queue.push_back(task_id);
            return true;
        }
        for wait_queue in &mut self.wait_queues {
            if take_task_id_by_id(wait_queue, task_id) {
                registry.finish_wait(task_id, TaskWaitResult::Woken);
                registry.mark_ready(task_id);
                self.ready_queue.push_back(task_id);
                return true;
            }
        }
        for wait_queue in &mut self.exit_wait_queues {
            if take_task_id_by_id(wait_queue, task_id) {
                registry.finish_wait(task_id, TaskWaitResult::Woken);
                registry.mark_ready(task_id);
                self.ready_queue.push_back(task_id);
                return true;
            }
        }
        false
    }

    fn wake_one_in_wait_queue(
        &mut self,
        registry: &mut TaskRegistry,
        wait_queue_id: WaitQueueId,
    ) -> Option<TaskId> {
        let task_id = self.wait_queue_mut(wait_queue_id).pop_front()?;
        registry.finish_wait(task_id, TaskWaitResult::Woken);
        registry.mark_ready(task_id);
        self.ready_queue.push_back(task_id);
        Some(task_id)
    }

    fn wake_all_in_wait_queue(
        &mut self,
        registry: &mut TaskRegistry,
        wait_queue_id: WaitQueueId,
    ) -> usize {
        let mut woken = 0usize;
        while let Some(task_id) = self.wait_queue_mut(wait_queue_id).pop_front() {
            registry.finish_wait(task_id, TaskWaitResult::Woken);
            registry.mark_ready(task_id);
            self.ready_queue.push_back(task_id);
            woken = woken.saturating_add(1);
        }
        woken
    }

    fn wait_queue_mut(&mut self, wait_queue_id: WaitQueueId) -> &mut VecDeque<TaskId> {
        self.wait_queues
            .get_mut(wait_queue_id)
            .expect("wait queue must exist before use")
    }

    fn exit_wait_queue_mut(&mut self, task_id: TaskId) -> &mut VecDeque<TaskId> {
        if self.exit_wait_queues.len() <= task_id {
            self.exit_wait_queues.resize_with(task_id + 1, VecDeque::new);
        }
        &mut self.exit_wait_queues[task_id]
    }

    fn wait_list_mut(&mut self, wait_handle: TaskWaitHandle) -> &mut VecDeque<TaskId> {
        match wait_handle.target() {
            TaskWaitTarget::WaitQueue(wait_queue_id) => self.wait_queue_mut(wait_queue_id),
            TaskWaitTarget::TaskExit(task_id) => self.exit_wait_queue_mut(task_id),
        }
    }

    fn remove_from_wait_list(&mut self, wait_handle: TaskWaitHandle, task_id: TaskId) -> bool {
        take_task_id_by_id(self.wait_list_mut(wait_handle), task_id)
    }

    fn wake_all_waiters_for_task_exit(&mut self, registry: &mut TaskRegistry, task_id: TaskId) {
        while let Some(waiter_task_id) = self.exit_wait_queue_mut(task_id).pop_front() {
            registry.finish_wait(waiter_task_id, TaskWaitResult::Woken);
            registry.mark_ready(waiter_task_id);
            self.ready_queue.push_back(waiter_task_id);
        }
    }

    fn reap_exited_task(&mut self, registry: &mut TaskRegistry, task_id: TaskId) -> Option<ExitedTask> {
        if !take_task_id_by_id(&mut self.exited_queue, task_id) {
            return None;
        }
        registry.reap_task(task_id)
    }

    fn reap_one_exited_task(&mut self, registry: &mut TaskRegistry) -> Option<ExitedTask> {
        let task_id = self.exited_queue.pop_front()?;
        registry.reap_task(task_id)
    }
}

struct RoundRobinScheduler {
    registry: TaskRegistry,
    queues: RoundRobinQueues,
}

impl RoundRobinScheduler {
    fn new() -> Self {
        Self {
            registry: TaskRegistry::new(),
            queues: RoundRobinQueues::new(),
        }
    }

    fn init(&mut self) {
        self.registry.init();
        self.queues.init();
    }

    fn spawn_kernel_task(&mut self, entry: KernelTaskEntry, arg: usize) -> TaskId {
        let task_id = self.registry.spawn_kernel_task(entry, arg);
        self.queues.push_spawned_task(task_id);
        log::debug!("[task-scheduler] spawned task {}", task_id);
        task_id
    }

    fn spawn_user_task(&mut self, entry_pc: UserTaskEntryPc) -> TaskId {
        let task_id = self.registry.spawn_user_task(entry_pc);
        self.queues.push_spawned_task(task_id);
        log::debug!("[task-scheduler] spawned user task {}", task_id);
        task_id
    }

    fn allocate_wait_queue(&mut self) -> WaitQueueId { self.queues.allocate_wait_queue() }

    fn prepare_first_switch(&mut self) -> SwitchPair {
        self.queues.promote_sleeping_tasks(&mut self.registry);
        self.queues.promote_wait_timeouts(&mut self.registry);
        let next_task_id = self.queues.pick_next_task_id();
        self.registry.first_switch_to(next_task_id)
    }

    fn schedule(&mut self, reason: ScheduleReason) -> Option<SwitchPair> {
        match reason {
            ScheduleReason::Tick => {
                self.queues.on_tick();
                self.registry.account_tick_for_current();
            }
            ScheduleReason::Sleep(ticks) if ticks == 0 => {
                return self.schedule(ScheduleReason::Yield);
            }
            _ => {}
        }

        self.queues.promote_sleeping_tasks(&mut self.registry);
        self.queues.promote_wait_timeouts(&mut self.registry);

        let (current_task_id, current_ptr) = self.registry.take_current_switch_out()?;

        if self.registry.is_idle(current_task_id) {
            let next_task_id = self.queues.pick_next_task_id();
            if next_task_id == current_task_id {
                let _ = self.registry.mark_running_and_set_current(next_task_id);
                return None;
            }
            let next_ptr = self.registry.mark_running_and_set_current(next_task_id);
            return Some((current_ptr, next_ptr));
        }

        let queue_target = match reason {
            ScheduleReason::StartFirst => QueueTarget::Ready,
            ScheduleReason::Yield | ScheduleReason::Tick => QueueTarget::Ready,
            ScheduleReason::Block(block_reason) => QueueTarget::Blocked(block_reason),
            ScheduleReason::Sleep(ticks) => {
                let wake_tick = self.queues.current_tick().saturating_add(ticks.max(1));
                QueueTarget::Sleeping(wake_tick)
            }
            ScheduleReason::Exit(exit_code) => QueueTarget::Exited(exit_code),
        };

        self.queues
            .enqueue_task(&mut self.registry, current_task_id, queue_target);

        let next_task_id = self.queues.pick_next_task_id();
        if next_task_id == current_task_id {
            let _ = self.registry.mark_running_and_set_current(next_task_id);
            return None;
        }

        let next_ptr = self.registry.mark_running_and_set_current(next_task_id);
        Some((current_ptr, next_ptr))
    }

    fn schedule_wait(
        &mut self,
        wait_handle: TaskWaitHandle,
        timeout_ticks: Option<TaskTick>,
    ) -> Option<SwitchPair> {
        self.queues.promote_sleeping_tasks(&mut self.registry);
        self.queues.promote_wait_timeouts(&mut self.registry);

        if self.registry.wait_target_ready(wait_handle) {
            if let Some(current_task_id) = self.registry.current_task_id() {
                self.registry.finish_wait(current_task_id, TaskWaitResult::Woken);
            }
            return None;
        }

        let (current_task_id, current_ptr) = self.registry.take_current_switch_out()?;
        self.registry.clear_wait_result(current_task_id);
        self.queues.enqueue_task(
            &mut self.registry,
            current_task_id,
            QueueTarget::Blocked(TaskBlockReason::Wait(wait_handle)),
        );
        if let Some(timeout_ticks) = timeout_ticks {
            let wake_tick = self.queues.current_tick().saturating_add(timeout_ticks.max(1));
            self.queues
                .enqueue_wait_timeout(current_task_id, wait_handle, wake_tick);
        }

        let next_task_id = self.queues.pick_next_task_id();
        let next_ptr = self.registry.mark_running_and_set_current(next_task_id);
        Some((current_ptr, next_ptr))
    }

    fn wake_task(&mut self, task_id: TaskId) -> bool {
        self.queues.wake_task(&mut self.registry, task_id)
    }

    fn reap_exited_task(&mut self, task_id: TaskId) -> Option<ExitedTask> {
        self.queues.reap_exited_task(&mut self.registry, task_id)
    }

    fn reap_one_exited_task(&mut self) -> Option<ExitedTask> {
        self.queues.reap_one_exited_task(&mut self.registry)
    }

    fn wake_one_in_wait_queue(&mut self, wait_queue_id: WaitQueueId) -> Option<TaskId> {
        self.queues
            .wake_one_in_wait_queue(&mut self.registry, wait_queue_id)
    }

    fn wake_all_in_wait_queue(&mut self, wait_queue_id: WaitQueueId) -> usize {
        self.queues
            .wake_all_in_wait_queue(&mut self.registry, wait_queue_id)
    }

    fn current_task_id(&self) -> Option<TaskId> { self.registry.current_task_id() }

    fn current_task_snapshot(&self) -> Option<TaskSnapshot> {
        self.registry.current_task_snapshot()
    }

    fn current_task_kernel_stack_top(&self) -> Option<usize> {
        self.registry.current_task_kernel_stack_top()
    }

    fn record_current_trap_frame(&mut self, trap_frame: TaskTrapFrame) {
        self.registry.record_current_trap_frame(trap_frame);
    }

    fn begin_current_trap_frame_access(
        &mut self,
        trap_frame: TaskTrapFrame,
    ) -> Option<*mut TaskTrapFrame> {
        self.registry.begin_current_trap_frame_access(trap_frame)
    }

    fn restore_current_trap_frame(&self, trap_frame: &mut TaskTrapFrame) -> bool {
        self.registry.restore_current_trap_frame(trap_frame)
    }

    fn take_current_wait_result(&mut self) -> TaskWaitResult { self.registry.take_current_wait_result() }
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
