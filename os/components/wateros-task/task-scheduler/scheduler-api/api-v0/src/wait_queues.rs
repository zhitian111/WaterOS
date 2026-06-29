//! 与具体就绪 run-queue 算法无关的等待/阻塞/睡眠/退出队列。

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::vec::Vec;
use task_api::{
    ExitedTask, TaskBlockReason, TaskExitCode, TaskId, TaskState, TaskTick, TaskWaitHandle,
    TaskWaitResult, TaskWaitTarget, WaitQueueId,
};

use crate::{QueueTarget, ReadyTaskSink, TaskRegistry};

#[derive(Clone, Copy)]
struct WaitTimeoutEntry {
    task_id : TaskId,
    wait_handle : TaskWaitHandle,
    wake_tick : TaskTick,
}

/// 阻塞、睡眠、等待与退出队列；就绪任务通过 `ready_queue` 参数回注具体 run-queue。
pub struct WaitQueues {
    wait_queues : Vec<VecDeque<TaskId>>,
    free_wait_queues : BTreeSet<WaitQueueId>,
    exit_wait_queues : BTreeMap<TaskId, VecDeque<TaskId>>,
    child_exit_wait_queues : BTreeMap<TaskId, VecDeque<TaskId>>,
    wait_timeouts : VecDeque<WaitTimeoutEntry>,
    blocked_queue : VecDeque<TaskId>,
    sleep_queue : VecDeque<TaskId>,
    exited_queue : VecDeque<TaskId>,
    current_tick : TaskTick,
}

impl WaitQueues {
    /// 构造空队列集。
    pub fn new() -> Self {
        Self { wait_queues : Vec::new(),
               free_wait_queues : BTreeSet::new(),
               exit_wait_queues : BTreeMap::new(),
               child_exit_wait_queues : BTreeMap::new(),
               wait_timeouts : VecDeque::new(),
               blocked_queue : VecDeque::new(),
               sleep_queue : VecDeque::new(),
               exited_queue : VecDeque::new(),
               current_tick : 0 }
    }

    /// 重置全部队列与逻辑 tick。
    pub fn init(&mut self) {
        self.wait_queues
            .clear();
        self.free_wait_queues
            .clear();
        self.exit_wait_queues
            .clear();
        self.child_exit_wait_queues
            .clear();
        self.wait_timeouts
            .clear();
        self.blocked_queue
            .clear();
        self.sleep_queue
            .clear();
        self.exited_queue
            .clear();
        self.current_tick = 0;
    }

    /// 分配新的显式等待队列 id。
    pub fn allocate_wait_queue(&mut self) -> WaitQueueId {
        if let Some(wait_queue_id) = self.free_wait_queues.pop_first() {
            self.wait_queues[wait_queue_id].clear();
            return wait_queue_id;
        }
        let wait_queue_id = self.wait_queues
                                .len();
        self.wait_queues
            .push(VecDeque::new());
        wait_queue_id
    }

    /// 释放一个当前没有等待者的显式等待队列；失败说明队列仍在使用或 id 非法。
    pub fn try_release_wait_queue(&mut self, wait_queue_id : WaitQueueId) -> bool {
        let Some(queue) = self.wait_queues.get(wait_queue_id) else {
            return false;
        };
        if !queue.is_empty() || !self.free_wait_queues.insert(wait_queue_id) {
            return false;
        }
        let handle = TaskWaitHandle::for_wait_queue(wait_queue_id);
        self.wait_timeouts
            .retain(|entry| entry.wait_handle != handle);
        true
    }

    #[cfg(test)]
    pub fn wait_queue_slot_count(&self) -> usize { self.wait_queues.len() }

    /// 推进全局逻辑 tick。
    pub fn on_tick(&mut self) {
        self.current_tick = self.current_tick
                                .saturating_add(1);
    }

    /// 当前逻辑 tick。
    pub fn current_tick(&self) -> TaskTick { self.current_tick }

    /// 是否存在已到期的 sleep 或 wait timeout（O(1) 队首探测，不移动元素）。
    pub fn has_due_timers(&self, registry : &TaskRegistry) -> bool {
        if self.wait_timeouts
              .front()
              .is_some_and(|entry| entry.wake_tick <= self.current_tick)
        {
            return true;
        }
        self.sleep_queue
            .front()
            .is_some_and(|&task_id| {
                matches!(
                    registry.state(task_id),
                    Some(TaskState::Sleeping { wake_tick }) if wake_tick <= self.current_tick
                )
            })
    }

    /// 将任务从一切可运行/等待队列中移除（不含 `exited_queue` 与 `ready_queue`）。
    pub fn detach_task_from_run_queues(&mut self, task_id : TaskId) {
        let _ = take_task_id_by_id(&mut self.blocked_queue, task_id);
        let _ = take_task_id_by_id(&mut self.sleep_queue, task_id);
        for wait_queue in &mut self.wait_queues {
            let _ = take_task_id_by_id(wait_queue, task_id);
        }
        for wait_queue in self.exit_wait_queues
                              .values_mut()
        {
            let _ = take_task_id_by_id(wait_queue, task_id);
        }
        for wait_queue in self.child_exit_wait_queues
                              .values_mut()
        {
            let _ = take_task_id_by_id(wait_queue, task_id);
        }
        self.exit_wait_queues
            .remove(&task_id);
        self.child_exit_wait_queues
            .remove(&task_id);
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

    /// 按目标将任务挂入对应队列；`Ready` 时写入 `ready_queue`。
    pub fn enqueue_task(&mut self,
                        registry : &mut TaskRegistry,
                        task_id : TaskId,
                        target : QueueTarget,
                        ready_queue : &mut impl ReadyTaskSink) {
        match target {
            QueueTarget::Ready => {
                if registry.state(task_id)
                           .is_none()
                {
                    return;
                }
                registry.mark_ready(task_id);
                ready_queue.enqueue_ready_task(task_id);
            }
            QueueTarget::Blocked(reason) => {
                if registry.state(task_id)
                           .is_none()
                {
                    return;
                }
                registry.mark_blocking(task_id, reason);
                match reason {
                    TaskBlockReason::Wait(wait_handle) => {
                        self.wait_list_mut(wait_handle)
                            .push_back(task_id);
                    }
                    _ => self.blocked_queue
                             .push_back(task_id),
                }
            }
            QueueTarget::Sleeping(wake_tick) => {
                if registry.state(task_id)
                           .is_none()
                {
                    return;
                }
                registry.mark_sleeping(task_id, wake_tick);
                let insert_at = self.sleep_queue
                                    .iter()
                                    .position(|queued_id| {
                                        matches!(
                                            registry.state(*queued_id),
                                            Some(TaskState::Sleeping {
                                                wake_tick: queued_tick
                                            }) if queued_tick > wake_tick
                                        )
                                    })
                                    .unwrap_or(self.sleep_queue
                                                   .len());
                self.sleep_queue
                    .insert(insert_at, task_id);
            }
            QueueTarget::Exited(exit_code) => {
                if registry.state(task_id)
                           .is_none()
                {
                    return;
                }
                self.wake_all_waiters_for_task_exit(registry, task_id, ready_queue);
                self.detach_task_from_run_queues(task_id);
                ready_queue.detach_ready_task(task_id);
                registry.mark_exited(task_id, exit_code);
                self.exited_queue
                    .push_back(task_id);
            }
        }
    }

    /// 记录带超时的等待项。
    pub fn enqueue_wait_timeout(&mut self,
                                task_id : TaskId,
                                wait_handle : TaskWaitHandle,
                                wake_tick : TaskTick) {
        let entry = WaitTimeoutEntry { task_id,
                                       wait_handle,
                                       wake_tick };
        let insert_at = self.wait_timeouts
                            .iter()
                            .position(|queued| queued.wake_tick > wake_tick)
                            .unwrap_or(self.wait_timeouts
                                           .len());
        self.wait_timeouts
            .insert(insert_at, entry);
    }

    /// 将到期的睡眠任务移入 `ready_queue`。
    pub fn promote_sleeping_tasks(&mut self,
                                  registry : &mut TaskRegistry,
                                  ready_queue : &mut impl ReadyTaskSink) {
        while let Some(task_id) = self.sleep_queue
                                      .front()
                                      .copied()
        {
            match registry.state(task_id) {
                None => {
                    self.sleep_queue
                        .pop_front();
                }
                Some(TaskState::Sleeping { wake_tick }) if wake_tick <= self.current_tick => {
                    self.sleep_queue
                        .pop_front();
                    registry.mark_ready(task_id);
                    ready_queue.enqueue_ready_task(task_id);
                    log::trace!("[task-scheduler] wake sleeping task {} at tick {}",
                                task_id,
                                self.current_tick);
                }
                Some(TaskState::Sleeping { .. }) => break,
                Some(_) => {
                    self.sleep_queue
                        .pop_front();
                }
            }
        }
    }

    /// 将到期的等待超时任务移入 `ready_queue`。
    pub fn promote_wait_timeouts(&mut self,
                                 registry : &mut TaskRegistry,
                                 ready_queue : &mut impl ReadyTaskSink) {
        while self.wait_timeouts
                  .front()
                  .is_some_and(|entry| entry.wake_tick <= self.current_tick)
        {
            let entry = self.wait_timeouts
                            .pop_front()
                            .expect("front entry checked above");
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
                ready_queue.enqueue_ready_task(entry.task_id);
            }
        }
    }

    /// 尝试唤醒指定任务。
    pub fn wake_task(&mut self,
                     registry : &mut TaskRegistry,
                     task_id : TaskId,
                     ready_queue : &mut impl ReadyTaskSink)
                     -> bool {
        self.finish_blocked_task(registry, task_id, ready_queue, TaskWaitResult::Woken)
    }

    /// 从任意等待/睡眠队列移除任务，并记录信号中断结果。
    pub fn interrupt_task(&mut self,
                          registry : &mut TaskRegistry,
                          task_id : TaskId,
                          ready_queue : &mut impl ReadyTaskSink)
                          -> bool {
        self.finish_blocked_task(registry, task_id, ready_queue, TaskWaitResult::Interrupted)
    }

    fn finish_blocked_task(&mut self,
                           registry : &mut TaskRegistry,
                           task_id : TaskId,
                           ready_queue : &mut impl ReadyTaskSink,
                           result : TaskWaitResult)
                           -> bool {
        self.wait_timeouts.retain(|entry| entry.task_id != task_id);
        if take_task_id_by_id(&mut self.blocked_queue, task_id) {
            if registry.state(task_id)
                       .is_none()
            {
                return false;
            }
            registry.finish_wait(task_id, result);
            registry.mark_ready(task_id);
            ready_queue.enqueue_ready_task(task_id);
            return true;
        }
        if take_task_id_by_id(&mut self.sleep_queue, task_id) {
            if registry.state(task_id)
                       .is_none()
            {
                return false;
            }
            registry.finish_wait(task_id, result);
            registry.mark_ready(task_id);
            ready_queue.enqueue_ready_task(task_id);
            return true;
        }
        for wait_queue in &mut self.wait_queues {
            if take_task_id_by_id(wait_queue, task_id) {
                if registry.state(task_id)
                           .is_none()
                {
                    return false;
                }
                registry.finish_wait(task_id, result);
                registry.mark_ready(task_id);
                ready_queue.enqueue_ready_task(task_id);
                return true;
            }
        }
        for wait_queue in self.exit_wait_queues
                              .values_mut()
        {
            if take_task_id_by_id(wait_queue, task_id) {
                if registry.state(task_id)
                           .is_none()
                {
                    return false;
                }
                registry.finish_wait(task_id, result);
                registry.mark_ready(task_id);
                ready_queue.enqueue_ready_task(task_id);
                return true;
            }
        }
        for wait_queue in self.child_exit_wait_queues
                              .values_mut()
        {
            if take_task_id_by_id(wait_queue, task_id) {
                if registry.state(task_id)
                           .is_none()
                {
                    return false;
                }
                registry.finish_wait(task_id, result);
                registry.mark_ready(task_id);
                ready_queue.enqueue_ready_task(task_id);
                return true;
            }
        }
        false
    }

    /// 将非当前任务标记为已退出。
    pub fn kill_task(&mut self,
                     registry : &mut TaskRegistry,
                     task_id : TaskId,
                     exit_code : TaskExitCode,
                     ready_queue : &mut impl ReadyTaskSink)
                     -> bool {
        use task_api::IDLE_TASK_ID;
        if task_id == IDLE_TASK_ID || registry.is_idle(task_id) {
            return false;
        }
        if registry.state(task_id)
                   .is_none()
        {
            return false;
        }
        if matches!(registry.state(task_id),
                    Some(TaskState::Exited(_)))
        {
            return true;
        }
        if registry.current_task_id() == Some(task_id) {
            return false;
        }
        self.enqueue_task(registry,
                          task_id,
                          QueueTarget::Exited(exit_code),
                          ready_queue);
        true
    }

    /// 从显式等待队列唤醒一个任务。
    pub fn wake_one_in_wait_queue(&mut self,
                                  registry : &mut TaskRegistry,
                                  wait_queue_id : WaitQueueId,
                                  ready_queue : &mut impl ReadyTaskSink)
                                  -> Option<TaskId> {
        while let Some(task_id) = self.wait_queue_mut(wait_queue_id)
                                      .pop_front()
        {
            if registry.state(task_id)
                       .is_none()
            {
                continue;
            }
            registry.finish_wait(task_id, TaskWaitResult::Woken);
            registry.mark_ready(task_id);
            ready_queue.enqueue_ready_task(task_id);
            return Some(task_id);
        }
        None
    }

    /// 清空显式等待队列并唤醒全部任务。
    pub fn wake_all_in_wait_queue(&mut self,
                                  registry : &mut TaskRegistry,
                                  wait_queue_id : WaitQueueId,
                                  ready_queue : &mut impl ReadyTaskSink)
                                  -> usize {
        let mut woken = 0usize;
        while let Some(task_id) = self.wait_queue_mut(wait_queue_id)
                                      .pop_front()
        {
            if registry.state(task_id)
                       .is_none()
            {
                continue;
            }
            registry.finish_wait(task_id, TaskWaitResult::Woken);
            registry.mark_ready(task_id);
            ready_queue.enqueue_ready_task(task_id);
            woken = woken.saturating_add(1);
        }
        woken
    }

    /// 从一个显式等待队列唤醒前 `wake_count` 个任务，并把后续最多
    /// `requeue_count` 个仍在等待的任务迁移到另一个等待队列。
    pub fn requeue_wait_queue(&mut self,
                              registry : &mut TaskRegistry,
                              from_wait_queue_id : WaitQueueId,
                              to_wait_queue_id : WaitQueueId,
                              wake_count : usize,
                              requeue_count : usize,
                              ready_queue : &mut impl ReadyTaskSink)
                              -> usize {
        let mut changed = 0usize;
        for _ in 0..wake_count {
            if self.wake_one_in_wait_queue(registry, from_wait_queue_id, ready_queue)
                   .is_none()
            {
                return changed;
            }
            changed = changed.saturating_add(1);
        }

        let from_handle = TaskWaitHandle::for_wait_queue(from_wait_queue_id);
        let to_handle = TaskWaitHandle::for_wait_queue(to_wait_queue_id);
        if from_wait_queue_id == to_wait_queue_id {
            let remaining = self.wait_queue_mut(from_wait_queue_id)
                                .iter()
                                .filter(|task_id| {
                                    matches!(
                                        registry.state(**task_id),
                                        Some(TaskState::Blocking(TaskBlockReason::Wait(handle)))
                                            if handle == from_handle
                                    )
                                })
                                .count()
                                .min(requeue_count);
            return changed.saturating_add(remaining);
        }

        let mut moved = 0usize;
        while moved < requeue_count {
            let Some(task_id) = self.wait_queue_mut(from_wait_queue_id)
                                    .pop_front()
            else {
                break;
            };
            match registry.state(task_id) {
                Some(TaskState::Blocking(TaskBlockReason::Wait(handle)))
                    if handle == from_handle =>
                {
                    registry.mark_blocking(task_id, TaskBlockReason::Wait(to_handle));
                    self.update_wait_timeout_handle(task_id, from_handle, to_handle);
                    self.wait_queue_mut(to_wait_queue_id)
                        .push_back(task_id);
                    moved = moved.saturating_add(1);
                    changed = changed.saturating_add(1);
                }
                Some(_) | None => {}
            }
        }
        changed
    }

    /// 回收指定已退出任务。
    pub fn reap_exited_task(&mut self,
                            registry : &mut TaskRegistry,
                            task_id : TaskId)
                            -> Option<ExitedTask> {
        if !take_task_id_by_id(&mut self.exited_queue, task_id) {
            return None;
        }
        self.detach_task_from_run_queues(task_id);
        self.exit_wait_queues
            .remove(&task_id);
        self.child_exit_wait_queues
            .remove(&task_id);
        registry.reap_task(task_id)
    }

    /// 按 FIFO 回收一个已退出任务。
    pub fn reap_one_exited_task(&mut self, registry : &mut TaskRegistry) -> Option<ExitedTask> {
        while let Some(task_id) = self.exited_queue
                                      .pop_front()
        {
            self.detach_task_from_run_queues(task_id);
            self.exit_wait_queues
                .remove(&task_id);
            self.child_exit_wait_queues
                .remove(&task_id);
            if let Some(exited) = registry.reap_task(task_id) {
                return Some(exited);
            }
        }
        None
    }

    fn wait_queue_mut(&mut self, wait_queue_id : WaitQueueId) -> &mut VecDeque<TaskId> {
        self.wait_queues
            .get_mut(wait_queue_id)
            .expect("wait queue must exist before use")
    }

    fn exit_wait_queue_mut(&mut self, task_id : TaskId) -> &mut VecDeque<TaskId> {
        self.exit_wait_queues
            .entry(task_id)
            .or_insert_with(VecDeque::new)
    }

    fn wait_list_mut(&mut self, wait_handle : TaskWaitHandle) -> &mut VecDeque<TaskId> {
        match wait_handle.target() {
            TaskWaitTarget::WaitQueue(wait_queue_id) => self.wait_queue_mut(wait_queue_id),
            TaskWaitTarget::TaskExit(task_id) => self.exit_wait_queue_mut(task_id),
            TaskWaitTarget::ChildExit(parent_id) => self.child_exit_wait_queue_mut(parent_id),
        }
    }

    fn remove_from_wait_list(&mut self, wait_handle : TaskWaitHandle, task_id : TaskId) -> bool {
        take_task_id_by_id(self.wait_list_mut(wait_handle), task_id)
    }

    fn update_wait_timeout_handle(&mut self,
                                  task_id : TaskId,
                                  from_handle : TaskWaitHandle,
                                  to_handle : TaskWaitHandle) {
        for entry in &mut self.wait_timeouts {
            if entry.task_id == task_id && entry.wait_handle == from_handle {
                entry.wait_handle = to_handle;
            }
        }
    }

    fn wake_all_waiters_for_task_exit(&mut self,
                                      registry : &mut TaskRegistry,
                                      task_id : TaskId,
                                      ready_queue : &mut impl ReadyTaskSink) {
        while let Some(waiter_task_id) = self.exit_wait_queue_mut(task_id)
                                             .pop_front()
        {
            if registry.state(waiter_task_id)
                       .is_none()
            {
                continue;
            }
            registry.finish_wait(waiter_task_id, TaskWaitResult::Woken);
            registry.mark_ready(waiter_task_id);
            ready_queue.enqueue_ready_task(waiter_task_id);
        }
        if let Some(parent_id) = registry.parent_id(task_id) {
            self.wake_child_exit_waiters(registry, parent_id, ready_queue);
        }
    }

    pub fn wake_child_exit_waiters(&mut self,
                                   registry : &mut TaskRegistry,
                                   parent_id : TaskId,
                                   ready_queue : &mut impl ReadyTaskSink) {
        while let Some(waiter_task_id) = self.child_exit_wait_queue_mut(parent_id)
                                             .pop_front()
        {
            if registry.state(waiter_task_id)
                       .is_none()
            {
                continue;
            }
            registry.finish_wait(waiter_task_id, TaskWaitResult::Woken);
            registry.mark_ready(waiter_task_id);
            ready_queue.enqueue_ready_task(waiter_task_id);
        }
    }

    pub fn block_task_manual(&mut self,
                             registry : &mut TaskRegistry,
                             task_id : TaskId,
                             ready_queue : &mut impl ReadyTaskSink) {
        if registry.state(task_id)
                  .is_none()
        {
            return;
        }
        ready_queue.detach_ready_task(task_id);
        self.enqueue_task(registry,
                          task_id,
                          QueueTarget::Blocked(TaskBlockReason::Manual),
                          ready_queue);
    }

    fn child_exit_wait_queue_mut(&mut self, parent_id : TaskId) -> &mut VecDeque<TaskId> {
        self.child_exit_wait_queues
            .entry(parent_id)
            .or_insert_with(VecDeque::new)
    }
}

fn take_task_id_by_id(queue : &mut VecDeque<TaskId>, task_id : TaskId) -> bool {
    let old_len = queue.len();
    queue.retain(|candidate_task_id| *candidate_task_id != task_id);
    queue.len() != old_len
}

#[cfg(test)]
mod tests {
    use super::WaitQueues;

    #[test]
    fn allocate_release_wait_queue_does_not_grow_unbounded() {
        let mut queues = WaitQueues::new();
        const ITERATIONS : usize = 10_000;
        for _ in 0..ITERATIONS {
            let id = queues.allocate_wait_queue();
            assert!(queues.try_release_wait_queue(id));
        }
        assert!(
            queues.wait_queue_slot_count() <= 1,
            "wait_queues should reuse freed ids, got len={}",
            queues.wait_queue_slot_count()
        );
    }
}
