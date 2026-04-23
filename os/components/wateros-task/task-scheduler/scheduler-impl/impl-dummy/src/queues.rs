extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use task_api::{
    ExitedTask, TaskBlockReason, TaskExitCode, TaskId, TaskState, TaskTick,
    TaskWaitHandle, TaskWaitResult, TaskWaitTarget, WaitQueueId, IDLE_TASK_ID,
};

use crate::registry::TaskRegistry;

pub(super) enum QueueTarget {
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

pub(super) struct RoundRobinQueues {
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
    pub(super) fn new() -> Self {
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

    pub(super) fn init(&mut self) {
        self.wait_queues.clear();
        self.exit_wait_queues.clear();
        self.wait_timeouts.clear();
        self.ready_queue.clear();
        self.blocked_queue.clear();
        self.sleep_queue.clear();
        self.exited_queue.clear();
        self.current_tick = 0;
    }

    pub(super) fn allocate_wait_queue(&mut self) -> WaitQueueId {
        let wait_queue_id = self.wait_queues.len();
        self.wait_queues.push(VecDeque::new());
        wait_queue_id
    }

    pub(super) fn push_spawned_task(&mut self, task_id: TaskId) { self.ready_queue.push_back(task_id); }

    pub(super) fn on_tick(&mut self) { self.current_tick = self.current_tick.saturating_add(1); }

    pub(super) fn current_tick(&self) -> TaskTick { self.current_tick }

    pub(super) fn pick_next_task_id(&mut self) -> TaskId {
        self.ready_queue.pop_front().unwrap_or(IDLE_TASK_ID)
    }

    pub(super) fn enqueue_task(
        &mut self,
        registry: &mut TaskRegistry,
        task_id: TaskId,
        target: QueueTarget,
    ) {
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

    pub(super) fn enqueue_wait_timeout(
        &mut self,
        task_id: TaskId,
        wait_handle: TaskWaitHandle,
        wake_tick: TaskTick,
    ) {
        self.wait_timeouts.push_back(WaitTimeoutEntry {
            task_id,
            wait_handle,
            wake_tick,
        });
    }

    pub(super) fn promote_sleeping_tasks(&mut self, registry: &mut TaskRegistry) {
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

    pub(super) fn promote_wait_timeouts(&mut self, registry: &mut TaskRegistry) {
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

    pub(super) fn wake_task(&mut self, registry: &mut TaskRegistry, task_id: TaskId) -> bool {
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

    pub(super) fn wake_one_in_wait_queue(
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

    pub(super) fn wake_all_in_wait_queue(
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

    pub(super) fn reap_exited_task(
        &mut self,
        registry: &mut TaskRegistry,
        task_id: TaskId,
    ) -> Option<ExitedTask> {
        if !take_task_id_by_id(&mut self.exited_queue, task_id) {
            return None;
        }
        registry.reap_task(task_id)
    }

    pub(super) fn reap_one_exited_task(&mut self, registry: &mut TaskRegistry) -> Option<ExitedTask> {
        let task_id = self.exited_queue.pop_front()?;
        registry.reap_task(task_id)
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
