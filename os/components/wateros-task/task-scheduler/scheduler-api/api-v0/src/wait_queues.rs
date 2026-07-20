//! 与具体就绪 run-queue 算法无关的等待/阻塞/睡眠/退出队列。

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::vec::Vec;
use arch::task;
use task_api::{
    ExitedTask, TaskExitCode, TaskId, TaskState, TaskTick, TaskWaitResult, TaskWaitTarget,
    WaitQueueId,
};

use crate::{QueueTarget, ReadyTaskSink, TaskRegistry};

#[derive(Clone, Copy)]
struct TimeoutTask {
    task_id : TaskId,
    wait_target : TaskWaitTarget,
    wake_tick : TaskTick,
}

/// 阻塞、睡眠、等待与退出队列；就绪任务通过 `ready_queue` 参数回注具体 run-queue。
pub struct WaitQueues {
    // ▲ 显式等待队列（供锁、futex、pipe 等同步对象使用）
    wait_queues : Vec<VecDeque<TaskId>>, // 动态增长的等待队列数组索引 0: [task_5, task_8]      ← 第 0 号等待队列，5 和 8 在等
    free_wait_queues : BTreeSet<WaitQueueId>, // 已释放可复用的队列 ID

    // ▲ 等待特定任务退出
    exit_wait_queues : BTreeMap<TaskId, VecDeque<TaskId>>, // 等 task_id 退出，key = 10: [5, 8]    ← 任务 5 和 8 在等任务 10 退出
    child_exit_wait_queues : BTreeMap<TaskId, VecDeque<TaskId>>, // 等父任务子任务退出

    // ▲ 超时管理
    wait_timeouts : VecDeque<TimeoutTask>, // 带超时的等待项（按 wake_tick 排序）

    // ▲ 通用阻塞 / 睡眠 / 退出
    blocked_queue : VecDeque<TaskId>, // 通用阻塞队列
    sleep_queue : VecDeque<TaskId>,   // 睡眠队列（按 wake_tick 升序排列）
    exited_queue : VecDeque<TaskId>,  // 已退出、待回收的 FIFO
    current_tick : TaskTick,          // 调度器逻辑 tick
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
        if let Some(wait_queue_id) = self.free_wait_queues
                                         .pop_first()
        {
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
        let Some(queue) = self.wait_queues
                              .get(wait_queue_id)
        else {
            return false;
        };
        if !queue.is_empty() ||
           !self.free_wait_queues
                .insert(wait_queue_id)
        {
            return false;
        }
        let target = TaskWaitTarget::WaitQueue(wait_queue_id);
        // 移除所有等待该队列的超时项。retain:只保留闭包返回 true 的元素，删除返回 false 的。
        self.wait_timeouts
            .retain(|record| record.wait_target != target);
        true
    }

    /// 推进全局逻辑 tick。
    pub fn on_tick(&mut self) {
        self.current_tick = self.current_tick
                                .saturating_add(1);
    }

    /// 当前逻辑 tick。
    pub fn current_tick(&self) -> TaskTick { self.current_tick }

    /// 是否存在已到期的 sleep 或 wait timeout。队列是排序的，如果队首都没到期，后面的到期时间更晚，更不可能到期。
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

    /// 将任务从所有等待队列中移除
    pub fn detach_task_from_run_queues(&mut self, task_id : TaskId) {
        self.blocked_queue
            .retain(|&id| id != task_id);
        self.sleep_queue
            .retain(|&id| id != task_id);
        for wait_queue in &mut self.wait_queues {
            wait_queue.retain(|&id| id != task_id);
        }
        for wait_queue in self.exit_wait_queues
                              .values_mut()
        {
            wait_queue.retain(|&id| id != task_id);
        }
        for wait_queue in self.child_exit_wait_queues
                              .values_mut()
        {
            wait_queue.retain(|&id| id != task_id);
        }
        self.wait_timeouts
            .retain(|entry| entry.task_id != task_id);
        // 删除等待task_id 的task
        self.exit_wait_queues
            .remove(&task_id);
        self.child_exit_wait_queues
            .remove(&task_id);
    }

    /// 按目标将任务挂入对应队列；`Ready` 时写入 `ready_queue`。
    /// TODO: 职责划分不清，需要进一步拆分，`WaitQueues` 不应直接操作 `TaskRegistry`和 `ReadyTaskSink`，应由上层调度器逻辑处理。
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
                    TaskWaitTarget::WaitQueue(wait_queue_id) => {
                        self.wait_queue_mut(wait_queue_id)
                            .push_back(task_id);
                    }
                    TaskWaitTarget::TaskExit(task_id_exit) => {
                        self.exit_wait_queue_mut(task_id_exit)
                            .push_back(task_id);
                    }
                    TaskWaitTarget::ChildExit(parent_id) => {
                        self.child_exit_wait_queue_mut(parent_id)
                            .push_back(task_id);
                    }
                    TaskWaitTarget::Manual => self.blocked_queue
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
    pub fn enqueue_blocked_task(&mut self, task_id : TaskId, target : TaskWaitTarget) {
        match target {
            TaskWaitTarget::WaitQueue(wait_queue_id) => self.wait_queue_mut(wait_queue_id)
                                                            .push_back(task_id),
            TaskWaitTarget::ChildExit(parent_id) => self.child_exit_wait_queue_mut(parent_id)
                                                        .push_back(task_id),
            TaskWaitTarget::TaskExit(id) => self.exit_wait_queue_mut(id)
                                                .push_back(task_id),
            TaskWaitTarget::Manual => self.blocked_queue
                                          .push_back(task_id),
        }
    }
    /// 记录带超时的等待项。
    pub fn enqueue_wait_timeout(&mut self,
                                task_id : TaskId,
                                target : TaskWaitTarget,
                                wake_tick : TaskTick) {
        let entry = TimeoutTask { task_id,
                                  wait_target : target,
                                  wake_tick };
        //TODO: 这里可以优化为二分查找插入，避免 O(n) 的线性扫描。
        let insert_at = self.wait_timeouts
                            .iter()
                            .position(|queued| queued.wake_tick > wake_tick)
                            .unwrap_or(self.wait_timeouts
                                           .len());
        self.wait_timeouts
            .insert(insert_at, entry);
    }

    /// 将到期的睡眠任务移入 `ready_queue`。
    //TODO: 这里不处理ready_queue，应该由上层调度器逻辑处理。可以改成只返回到期的任务列表。
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
    //TODO: 这里不处理ready_queue，应该由上层调度器逻辑处理。可以改成只返回到期的任务列表。
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
                (registry.state(entry.task_id), entry.wait_target),
                (Some(TaskState::Blocking(target)), saved_target) if target == saved_target
            );
            if !still_waiting {
                continue;
            }
            if self.remove_from_wait_list(entry.wait_target, entry.task_id) {
                registry.finish_wait(entry.task_id, TaskWaitResult::TimedOut);
                registry.mark_ready(entry.task_id);
                ready_queue.enqueue_ready_task(entry.task_id);
            }
        }
    }
    pub fn wait_timesouts_front(&self) -> Option<TimeoutTask> {
        self.wait_timeouts
            .front()
            .copied()
    }

    /// 尝试唤醒指定任务。
    //TODO: 这里不处理ready_queue，应该由上层调度器逻辑处理。可以改成只返回是否唤醒成功。
    pub fn wake_task(&mut self,
                     registry : &mut TaskRegistry,
                     task_id : TaskId,
                     ready_queue : &mut impl ReadyTaskSink)
                     -> bool {
        self.finish_blocked_task(registry,
                                 task_id,
                                 ready_queue,
                                 TaskWaitResult::Woken)
    }

    /// 从任意等待/睡眠队列移除任务，并记录信号中断结果。
    //TODO: 这里不处理ready_queue，应该由上层调度器逻辑处理。可以改成只返回是否唤醒成功。
    pub fn interrupt_task(&mut self,
                          registry : &mut TaskRegistry,
                          task_id : TaskId,
                          ready_queue : &mut impl ReadyTaskSink)
                          -> bool {
        self.finish_blocked_task(registry,
                                 task_id,
                                 ready_queue,
                                 TaskWaitResult::Interrupted)
    }

    fn finish_blocked_task(&mut self,
                           registry : &mut TaskRegistry,
                           task_id : TaskId,
                           ready_queue : &mut impl ReadyTaskSink,
                           result : TaskWaitResult)
                           -> bool {
        self.wait_timeouts
            .retain(|entry| entry.task_id != task_id);
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
            if self.wake_one_in_wait_queue(registry,
                                           from_wait_queue_id,
                                           ready_queue)
                   .is_none()
            {
                return changed;
            }
            changed = changed.saturating_add(1);
        }

        if from_wait_queue_id == to_wait_queue_id {
            let remaining = self.wait_queue_mut(from_wait_queue_id)
                                .iter()
                                .filter(|task_id| {
                                    matches!(
                                        registry.state(**task_id),
                                        Some(TaskState::Blocking(
                                            TaskWaitTarget::WaitQueue(id)
                                        )) if id == from_wait_queue_id
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
                Some(TaskState::Blocking(TaskWaitTarget::WaitQueue(id)))
                    if id == from_wait_queue_id =>
                {
                    registry.mark_blocking(task_id,
                                           TaskWaitTarget::WaitQueue(to_wait_queue_id));
                    self.update_wait_timeout_target(task_id,
                                                    from_wait_queue_id,
                                                    to_wait_queue_id);
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

    fn wait_list_mut(&mut self, target : TaskWaitTarget) -> &mut VecDeque<TaskId> {
        match target {
            TaskWaitTarget::WaitQueue(wait_queue_id) => self.wait_queue_mut(wait_queue_id),
            TaskWaitTarget::TaskExit(task_id) => self.exit_wait_queue_mut(task_id),
            TaskWaitTarget::ChildExit(parent_id) => self.child_exit_wait_queue_mut(parent_id),
            TaskWaitTarget::Manual => &mut self.blocked_queue,
        }
    }

    fn remove_from_wait_list(&mut self, target : TaskWaitTarget, task_id : TaskId) -> bool {
        take_task_id_by_id(self.wait_list_mut(target), task_id)
    }

    fn update_wait_timeout_target(&mut self,
                                  task_id : TaskId,
                                  from_queue_id : WaitQueueId,
                                  to_queue_id : WaitQueueId) {
        for entry in &mut self.wait_timeouts {
            if entry.task_id == task_id &&
               entry.wait_target == TaskWaitTarget::WaitQueue(from_queue_id)
            {
                entry.wait_target = TaskWaitTarget::WaitQueue(to_queue_id);
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
                          QueueTarget::Blocked(TaskWaitTarget::Manual),
                          ready_queue);
    }

    fn child_exit_wait_queue_mut(&mut self, parent_id : TaskId) -> &mut VecDeque<TaskId> {
        self.child_exit_wait_queues
            .entry(parent_id)
            .or_insert_with(VecDeque::new)
    }
}
/// 从队列中移除指定任务；返回是否成功移除。retain只保留闭包返回 true 的元素，删掉返回 false 的元素
fn take_task_id_by_id(queue : &mut VecDeque<TaskId>, task_id : TaskId) -> bool {
    let old_len = queue.len();
    queue.retain(|candidate_task_id| *candidate_task_id != task_id);
    queue.len() != old_len
}
