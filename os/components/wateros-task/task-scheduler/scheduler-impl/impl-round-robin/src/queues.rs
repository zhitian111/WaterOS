//! 轮转调度器的 **队列与 tick 记账**：就绪 FIFO、按原因的阻塞分桶、睡眠与退出队列，以及等待超时扫描。
//!
//! 与 [`crate::registry::TaskRegistry`] 协同：`enqueue_task` 同时更新 TCB 状态与队列结构；`task_id` 与 `WaitQueueId` 均为稠密下标假设。

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use task_api::{
    ExitedTask, TaskBlockReason, TaskExitCode, TaskId, TaskState, TaskTick, TaskWaitHandle,
    TaskWaitResult, TaskWaitTarget, WaitQueueId, IDLE_TASK_ID,
};

use crate::registry::TaskRegistry;

/// 将当前任务从运行态移出后应进入的调度桶。
pub(super) enum QueueTarget {
    Ready,
    Blocked(TaskBlockReason),
    Sleeping(TaskTick),
    Exited(TaskExitCode),
}

/// 带截止 tick 的等待项；到期时若任务仍在同一 `Wait` 阻塞上则超时唤醒。
#[derive(Clone, Copy)]
struct WaitTimeoutEntry {
    task_id: TaskId,
    wait_handle: TaskWaitHandle,
    wake_tick: TaskTick,
}

/// 轮转实现中的全部队列与当前逻辑 tick；不持有 TCB，仅通过 `TaskRegistry` 查询/更新任务状态。
pub(super) struct RoundRobinQueues {
    // 下标为 `WaitQueueId` 的 FIFO；与 `TaskWaitTarget::WaitQueue` 对应。
    wait_queues: Vec<VecDeque<TaskId>>,
    // 下标为被等待任务的 `TaskId`；与 `TaskWaitTarget::TaskExit` 对应。
    exit_wait_queues: Vec<VecDeque<TaskId>>,
    // 下标为父任务 `TaskId`；与 `TaskWaitTarget::ChildExit` 对应。
    child_exit_wait_queues: Vec<VecDeque<TaskId>>,
    // 按入队顺序扫描；到期 tick 不大于 `current_tick` 时尝试超时唤醒。
    wait_timeouts: VecDeque<WaitTimeoutEntry>,
    ready_queue: VecDeque<TaskId>,
    // 非 `Wait` 类阻塞原因的兜底队列（与 wait 分桶并行存在）。
    blocked_queue: VecDeque<TaskId>,
    sleep_queue: VecDeque<TaskId>,
    exited_queue: VecDeque<TaskId>,
    // 全局逻辑时钟：每次 `Tick` 调度原因时递增。
    current_tick: TaskTick,
}

impl RoundRobinQueues {
    pub(super) fn new() -> Self {
        Self {
            wait_queues: Vec::new(),
            exit_wait_queues: Vec::new(),
            child_exit_wait_queues: Vec::new(),
            wait_timeouts: VecDeque::new(),
            ready_queue: VecDeque::new(),
            blocked_queue: VecDeque::new(),
            sleep_queue: VecDeque::new(),
            exited_queue: VecDeque::new(),
            current_tick: 0,
        }
    }

    pub(super) fn init(&mut self) {
        self.wait_queues
            .clear();
        self.exit_wait_queues
            .clear();
        self.child_exit_wait_queues
            .clear();
        self.wait_timeouts
            .clear();
        self.ready_queue
            .clear();
        self.blocked_queue
            .clear();
        self.sleep_queue
            .clear();
        self.exited_queue
            .clear();
        self.current_tick = 0;
    }

    pub(super) fn allocate_wait_queue(&mut self) -> WaitQueueId {
        let wait_queue_id = self
            .wait_queues
            .len();
        self.wait_queues
            .push(VecDeque::new());
        wait_queue_id
    }

    pub(super) fn push_spawned_task(&mut self, task_id: TaskId) {
        self.ready_queue
            .push_back(task_id);
    }

    pub(super) fn on_tick(&mut self) {
        self.current_tick = self
            .current_tick
            .saturating_add(1);
    }

    pub(super) fn current_tick(&self) -> TaskTick {
        self.current_tick
    }

    /// 从就绪队列中弹出第一个 **存在且未退出** 的任务；跳过 stale / zombie 项。
    pub(super) fn pick_next_runnable_task_id(&mut self, registry : &TaskRegistry) -> TaskId {
        while let Some(task_id) = self.ready_queue
                                        .pop_front()
        {
            if registry.is_schedulable(task_id) {
                return task_id;
            }
            log::trace!("[task-scheduler] skip unrunnable task {} in ready_queue",
                        task_id);
        }
        IDLE_TASK_ID
    }

    /// 将任务从一切 **可运行/等待** 队列中移除（不含 `exited_queue`）。
    pub(super) fn detach_task_from_run_queues(&mut self, task_id : TaskId) {
        let _ = take_task_id_by_id(&mut self.ready_queue, task_id);
        let _ = take_task_id_by_id(&mut self.blocked_queue, task_id);
        let _ = take_task_id_by_id(&mut self.sleep_queue, task_id);
        for wait_queue in &mut self.wait_queues {
            let _ = take_task_id_by_id(wait_queue, task_id);
        }
        for wait_queue in &mut self.exit_wait_queues {
            let _ = take_task_id_by_id(wait_queue, task_id);
        }
        for wait_queue in &mut self.child_exit_wait_queues {
            let _ = take_task_id_by_id(wait_queue, task_id);
        }
        let mut pending = VecDeque::new();
        while let Some(entry) = self.wait_timeouts
                                        .pop_front()
        {
            if entry.task_id != task_id {
                pending.push_back(entry);
            }
        }
        self.wait_timeouts = pending;
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
                self.ready_queue
                    .push_back(task_id);
            }
            QueueTarget::Blocked(reason) => {
                registry.mark_blocking(task_id, reason);
                match reason {
                    TaskBlockReason::Wait(wait_handle) => {
                        self.wait_list_mut(wait_handle)
                            .push_back(task_id);
                    }
                    _ => self
                        .blocked_queue
                        .push_back(task_id),
                }
            }
            QueueTarget::Sleeping(wake_tick) => {
                registry.mark_sleeping(task_id, wake_tick);
                self.sleep_queue
                    .push_back(task_id);
            }
            QueueTarget::Exited(exit_code) => {
                self.detach_task_from_run_queues(task_id);
                registry.mark_exited(task_id, exit_code);
                self.exited_queue
                    .push_back(task_id);
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
        self.wait_timeouts
            .push_back(WaitTimeoutEntry {
                task_id,
                wait_handle,
                wake_tick,
            });
    }

    pub(super) fn promote_sleeping_tasks(&mut self, registry: &mut TaskRegistry) {
        let mut still_sleeping = VecDeque::new();
        while let Some(task_id) = self
            .sleep_queue
            .pop_front()
        {
            if registry.ready_to_wake(task_id, self.current_tick) {
                registry.mark_ready(task_id);
                self.ready_queue
                    .push_back(task_id);
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
        while let Some(entry) = self
            .wait_timeouts
            .pop_front()
        {
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
                self.ready_queue
                    .push_back(entry.task_id);
            }
        }
        self.wait_timeouts = pending;
    }

    pub(super) fn wake_task(&mut self, registry: &mut TaskRegistry, task_id: TaskId) -> bool {
        if take_task_id_by_id(&mut self.blocked_queue, task_id) {
            registry.finish_wait(task_id, TaskWaitResult::Woken);
            registry.mark_ready(task_id);
            self.ready_queue
                .push_back(task_id);
            return true;
        }
        if take_task_id_by_id(&mut self.sleep_queue, task_id) {
            registry.finish_wait(task_id, TaskWaitResult::Woken);
            registry.mark_ready(task_id);
            self.ready_queue
                .push_back(task_id);
            return true;
        }
        for wait_queue in &mut self.wait_queues {
            if take_task_id_by_id(wait_queue, task_id) {
                registry.finish_wait(task_id, TaskWaitResult::Woken);
                registry.mark_ready(task_id);
                self.ready_queue
                    .push_back(task_id);
                return true;
            }
        }
        for wait_queue in &mut self.exit_wait_queues {
            if take_task_id_by_id(wait_queue, task_id) {
                registry.finish_wait(task_id, TaskWaitResult::Woken);
                registry.mark_ready(task_id);
                self.ready_queue
                    .push_back(task_id);
                return true;
            }
        }
        for wait_queue in &mut self.child_exit_wait_queues {
            if take_task_id_by_id(wait_queue, task_id) {
                registry.finish_wait(task_id, TaskWaitResult::Woken);
                registry.mark_ready(task_id);
                self.ready_queue
                    .push_back(task_id);
                return true;
            }
        }
        false
    }

    /// 将非当前任务标记为已退出；当前任务须由调用方走 `exit_current`。
    pub(super) fn kill_task(&mut self,
                           registry : &mut TaskRegistry,
                           task_id : TaskId,
                           exit_code : TaskExitCode)
                           -> bool {
        if task_id == IDLE_TASK_ID || registry.is_idle(task_id) {
            return false;
        }
        if registry.state(task_id).is_none() {
            return false;
        }
        if matches!(registry.state(task_id), Some(TaskState::Exited(_))) {
            return true;
        }
        if registry.current_task_id() == Some(task_id) {
            return false;
        }
        self.enqueue_task(registry, task_id, QueueTarget::Exited(exit_code));
        true
    }

    pub(super) fn wake_one_in_wait_queue(
        &mut self,
        registry: &mut TaskRegistry,
        wait_queue_id: WaitQueueId,
    ) -> Option<TaskId> {
        let task_id = self
            .wait_queue_mut(wait_queue_id)
            .pop_front()?;
        registry.finish_wait(task_id, TaskWaitResult::Woken);
        registry.mark_ready(task_id);
        self.ready_queue
            .push_back(task_id);
        Some(task_id)
    }

    pub(super) fn wake_all_in_wait_queue(
        &mut self,
        registry: &mut TaskRegistry,
        wait_queue_id: WaitQueueId,
    ) -> usize {
        let mut woken = 0usize;
        while let Some(task_id) = self
            .wait_queue_mut(wait_queue_id)
            .pop_front()
        {
            registry.finish_wait(task_id, TaskWaitResult::Woken);
            registry.mark_ready(task_id);
            self.ready_queue
                .push_back(task_id);
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
        self.detach_task_from_run_queues(task_id);
        registry.reap_task(task_id)
    }

    pub(super) fn reap_one_exited_task(
        &mut self,
        registry: &mut TaskRegistry,
    ) -> Option<ExitedTask> {
        let task_id = self
            .exited_queue
            .pop_front()?;
        self.detach_task_from_run_queues(task_id);
        registry.reap_task(task_id)
    }

    fn wait_queue_mut(&mut self, wait_queue_id: WaitQueueId) -> &mut VecDeque<TaskId> {
        self.wait_queues
            .get_mut(wait_queue_id)
            .expect("wait queue must exist before use")
    }

    fn exit_wait_queue_mut(&mut self, task_id: TaskId) -> &mut VecDeque<TaskId> {
        if self
            .exit_wait_queues
            .len()
            <= task_id
        {
            self.exit_wait_queues
                .resize_with(task_id + 1, VecDeque::new);
        }
        &mut self.exit_wait_queues[task_id]
    }

    fn wait_list_mut(&mut self, wait_handle: TaskWaitHandle) -> &mut VecDeque<TaskId> {
        match wait_handle.target() {
            TaskWaitTarget::WaitQueue(wait_queue_id) => self.wait_queue_mut(wait_queue_id),
            TaskWaitTarget::TaskExit(task_id) => self.exit_wait_queue_mut(task_id),
            TaskWaitTarget::ChildExit(parent_id) => self.child_exit_wait_queue_mut(parent_id),
        }
    }

    fn remove_from_wait_list(&mut self, wait_handle: TaskWaitHandle, task_id: TaskId) -> bool {
        take_task_id_by_id(self.wait_list_mut(wait_handle), task_id)
    }

    fn wake_all_waiters_for_task_exit(&mut self, registry: &mut TaskRegistry, task_id: TaskId) {
        while let Some(waiter_task_id) = self
            .exit_wait_queue_mut(task_id)
            .pop_front()
        {
            registry.finish_wait(waiter_task_id, TaskWaitResult::Woken);
            registry.mark_ready(waiter_task_id);
            self.ready_queue
                .push_back(waiter_task_id);
        }
        if let Some(parent_id) = registry.parent_id(task_id) {
            while let Some(waiter_task_id) = self
                .child_exit_wait_queue_mut(parent_id)
                .pop_front()
            {
                registry.finish_wait(waiter_task_id, TaskWaitResult::Woken);
                registry.mark_ready(waiter_task_id);
                self.ready_queue
                    .push_back(waiter_task_id);
            }
        }
    }

    fn child_exit_wait_queue_mut(&mut self, parent_id: TaskId) -> &mut VecDeque<TaskId> {
        if self
            .child_exit_wait_queues
            .len()
            <= parent_id
        {
            self.child_exit_wait_queues
                .resize_with(parent_id + 1, VecDeque::new);
        }
        &mut self.child_exit_wait_queues[parent_id]
    }
}

// 从 deque 中精确移除第一个匹配的 `task_id`，保持其余元素顺序；O(n) 扫描，符合当前 bring-up 规模假设。
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
