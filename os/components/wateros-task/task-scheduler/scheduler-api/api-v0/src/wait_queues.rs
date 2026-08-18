//! 与具体就绪 run-queue 算法无关的等待/阻塞/睡眠/退出队列。

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::vec::Vec;
use task_api::{ExitedTask, TaskId, TaskTick, TaskWaitTarget, WaitQueueId};

use crate::TaskRegistry;

#[derive(Clone, Copy)]
struct TimeoutTask {
    /// 等待任务。
    task_id : TaskId,
    /// 等待目标，用于唤醒时校验原因。
    wait_target : TaskWaitTarget,
    /// 绝对唤醒 tick。
    wake_tick : TaskTick,
}

/// 阻塞、睡眠、等待与退出队列；就绪任务通过 `ready_queue` 参数回注具体 run-queue。
pub struct WaitQueues {
    // ▲ 显式等待队列（供锁、futex、pipe 等同步对象使用）
    wait_queues : Vec<VecDeque<TaskId>>, // 动态增长的等待队列数组索引 0: [task_5, task_8]      ← 第 0 号等待队列，5 和 8 在等
    wait_queue_names : Vec<Option<&'static str>>, // 仅用于诊断等待来源，不参与调度语义
    free_wait_queues : BTreeSet<WaitQueueId>, // 已释放可复用的队列 ID

    // ▲ 等待特定任务退出
    exit_wait_queues : BTreeMap<TaskId, VecDeque<TaskId>>, // 等 task_id 退出，key = 10: [5, 8]    ← 任务 5 和 8 在等任务 10 退出
    child_exit_wait_queues : BTreeMap<TaskId, VecDeque<TaskId>>, // 等父任务子任务退出

    // ▲ 超时管理
    wait_timeouts : VecDeque<TimeoutTask>, // 带超时的等待项（按 wake_tick 排序）

    // ▲ 通用阻塞 / 睡眠 / 退出
    blocked_queue : VecDeque<TaskId>,           // 通用阻塞队列
    sleep_queue : VecDeque<(TaskId, TaskTick)>, // 睡眠队列（按 wake_tick 升序排列）
    exited_queue : VecDeque<TaskId>,            // 已退出、待回收的 FIFO
    current_tick : TaskTick,                    // 调度器逻辑 tick
}

impl WaitQueues {
    /// 构造空队列集。
    pub fn new() -> Self {
        Self { wait_queues : Vec::new(),
               wait_queue_names : Vec::new(),
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
        self.wait_queue_names
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
    pub fn allocate_wait_queue(&mut self, name : &'static str) -> WaitQueueId {
        if let Some(wait_queue_id) = self.free_wait_queues
                                         .pop_first()
        {
            self.wait_queues[wait_queue_id].clear();
            self.wait_queue_names[wait_queue_id] = Some(name);
            return wait_queue_id;
        }
        let wait_queue_id = self.wait_queues
                                .len();
        self.wait_queues
            .push(VecDeque::new());
        self.wait_queue_names
            .push(Some(name));
        wait_queue_id
    }

    /// 返回等待队列的静态诊断标签；队列已释放或编号非法时返回 `None`。
    pub fn wait_queue_name(&self, wait_queue_id : WaitQueueId) -> Option<&'static str> {
        self.wait_queue_names
            .get(wait_queue_id)
            .copied()
            .flatten()
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
        self.wait_queue_names[wait_queue_id] = None;
        // 移除所有等待该队列的超时项。retain:只保留闭包返回 true 的元素，删除返回 false 的。
        self.wait_timeouts
            .retain(|record| record.wait_target != target);
        true
    }

    /// 推进全局逻辑 tick。
    pub fn tick(&mut self) {
        self.current_tick = self.current_tick
                                .saturating_add(1);
    }

    /// 当前逻辑 tick。
    pub fn current_tick(&self) -> TaskTick { self.current_tick }

    /// 是否存在已到期的 sleep 或 wait timeout。队列是排序的，如果队首都没到期，后面的到期时间更晚，更不可能到期。
    pub fn has_woken_or_timeout_tasks(&self) -> bool {
        if self.wait_timeouts
               .front()
               .is_some_and(|entry| entry.wake_tick <= self.current_tick)
        {
            return true;
        }
        self.sleep_queue
            .front()
            .is_some_and(|&(_, wake_tick)| wake_tick <= self.current_tick)
    }

    /// 将任务从所有等待队列中移除
    pub fn detach_task_from_run_queues(&mut self, task_id : TaskId) {
        self.blocked_queue
            .retain(|&id| id != task_id);
        self.sleep_queue
            .retain(|(id, _)| *id != task_id);
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

    /// 将任务入队到等待队列。
    pub fn enqueue_wait_task(&mut self, task_id : TaskId, target : TaskWaitTarget) {
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
    pub fn enqueue_sleep_task(&mut self, task_id : TaskId, wake_tick : u64) {
        let insert_at = self.sleep_queue
                            .iter()
                            .position(|(_, queued_tick)| *queued_tick > wake_tick)
                            .unwrap_or(self.sleep_queue
                                           .len());
        self.sleep_queue
            .insert(insert_at, (task_id, wake_tick));
    }
    /// 将退出任务入队：从其他队列中清理掉该任务的残留，再推入退出队列。
    pub fn enqueue_exited_task(&mut self, task_id : TaskId) {
        self.detach_task_from_run_queues(task_id);
        self.exited_queue
            .push_back(task_id);
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

    /// 将到期的睡眠任务弹出，返回其 ID 列表（调度器负责标记就绪并入队）。
    pub fn woken_tasks(&mut self) -> VecDeque<TaskId> {
        let mut woken = VecDeque::new();
        while let Some(&(task_id, wake_tick)) = self.sleep_queue
                                                    .front()
        {
            if wake_tick <= self.current_tick {
                self.sleep_queue
                    .pop_front();
                woken.push_back(task_id);
            } else {
                break;
            }
        }
        woken
    }

    /// 将到期的等待超时任务弹出，返回其 (task_id, wait_target) 列表。
    /// 调度器负责检查状态、标记超时结果并重新入就绪队列。
    pub fn timeout_tasks(&mut self) -> VecDeque<(TaskId, TaskWaitTarget)> {
        let mut timed_out = VecDeque::new();
        while self.wait_timeouts
                  .front()
                  .is_some_and(|entry| entry.wake_tick <= self.current_tick)
        {
            let entry = self.wait_timeouts
                            .pop_front()
                            .expect("front entry checked above");
            // 超时与显式 wake 一样，必须先从实际等待队列摘除任务。否则队列中
            // 会残留已经 Ready/Running 的 TaskId，后续 wake/requeue 会再次激活
            // 同一任务并破坏调度状态。
            if self.remove_task_from_wait_target(entry.task_id, entry.wait_target) {
                timed_out.push_back((entry.task_id, entry.wait_target));
            }
        }
        timed_out
    }

    /// 从任意等待/睡眠队列移除指定任务，返回是否找到。
    /// （调度器负责 registry.finish_wait + mark_ready + 就绪入队）
    pub fn wake_task(&mut self, task_id : TaskId) -> bool {
        self.remove_task_from_any_queue(task_id)
    }

    /// 从任意等待/睡眠队列移除指定任务，返回是否找到。
    pub fn interrupt_task(&mut self, task_id : TaskId) -> bool {
        self.remove_task_from_any_queue(task_id)
    }

    /// 扫描所有阻塞/睡眠/等待队列，移除指定任务并清理超时记录。
    fn remove_task_from_any_queue(&mut self, task_id : TaskId) -> bool {
        self.wait_timeouts
            .retain(|entry| entry.task_id != task_id);
        if take_task_id_by_id(&mut self.blocked_queue, task_id) {
            return true;
        }
        if self.sleep_queue
               .iter()
               .position(|(id, _)| *id == task_id)
               .is_some_and(|pos| {
                   self.sleep_queue
                       .remove(pos);
                   true
               })
        {
            return true;
        }
        for wait_queue in &mut self.wait_queues {
            if take_task_id_by_id(wait_queue, task_id) {
                return true;
            }
        }
        for wait_queue in self.exit_wait_queues
                              .values_mut()
        {
            if take_task_id_by_id(wait_queue, task_id) {
                return true;
            }
        }
        for wait_queue in self.child_exit_wait_queues
                              .values_mut()
        {
            if take_task_id_by_id(wait_queue, task_id) {
                return true;
            }
        }
        false
    }

    /// 将非当前任务推入退出队列（调度器负责前置检查）。
    pub fn kill_task(&mut self, task_id : TaskId) {
        self.detach_task_from_run_queues(task_id);
        self.exited_queue
            .push_back(task_id);
    }

    /// 从显式等待队列弹出一个任务（调度器负责状态更新和就绪入队）。
    pub fn wake_one_in_wait_queue(&mut self, wait_queue_id : WaitQueueId) -> Option<TaskId> {
        let task_id = self.wait_queue_mut(wait_queue_id)
                          .pop_front()?;
        self.remove_wait_timeout(task_id,
                                 TaskWaitTarget::WaitQueue(wait_queue_id));
        Some(task_id)
    }

    /// 清空显式等待队列，返回所有任务 ID（调度器负责状态更新和就绪入队）。
    pub fn wake_all_in_wait_queue(&mut self, wait_queue_id : WaitQueueId) -> VecDeque<TaskId> {
        let mut woken = VecDeque::new();
        while let Some(task_id) = self.wait_queue_mut(wait_queue_id)
                                      .pop_front()
        {
            self.remove_wait_timeout(task_id,
                                     TaskWaitTarget::WaitQueue(wait_queue_id));
            woken.push_back(task_id);
        }
        woken
    }

    /// 从一个显式等待队列中唤醒前 `wake_count` 个任务（弹出返回），
    /// 并把后续最多 `requeue_count` 个任务迁移到另一个等待队列。
    /// 返回 (woken_ids, moved_task_and_from_queue_ids, changed_count)。
    /// 调度器负责 registry 状态更新和就绪入队。
    pub fn requeue_wait_queue(&mut self,
                              from_wait_queue_id : WaitQueueId,
                              to_wait_queue_id : WaitQueueId,
                              wake_count : usize,
                              requeue_count : usize)
                              -> (VecDeque<TaskId>, VecDeque<(TaskId, WaitQueueId)>, usize) {
        let mut changed = 0usize;

        // Phase 1: 弹出前 wake_count 个任务（已唤醒）
        let woken = self.wake_n_from_wait_queue(from_wait_queue_id,
                                                wake_count,
                                                &mut changed);

        // Phase 2: 同队列 → 直接返回，迁移数为剩余任务数（上限 requeue_count）
        if from_wait_queue_id == to_wait_queue_id {
            let remaining = self.wait_queue_mut(from_wait_queue_id)
                                .len()
                                .min(requeue_count);
            changed = changed.saturating_add(remaining);
            return (woken, VecDeque::new(), changed);
        }

        // Phase 3: 不同队列 → 最多迁移 requeue_count 个任务
        let moved = self.move_tasks_between_queues(from_wait_queue_id,
                                                   to_wait_queue_id,
                                                   requeue_count,
                                                   &mut changed);
        (woken, moved, changed)
    }

    /// 从显式等待队列弹出最多 `count` 个任务。
    fn wake_n_from_wait_queue(&mut self,
                              wait_queue_id : WaitQueueId,
                              count : usize,
                              changed : &mut usize)
                              -> VecDeque<TaskId> {
        let mut woken = VecDeque::new();
        for _ in 0..count {
            match self.wait_queue_mut(wait_queue_id)
                      .pop_front()
            {
                Some(task_id) => {
                    self.remove_wait_timeout(task_id,
                                             TaskWaitTarget::WaitQueue(wait_queue_id));
                    woken.push_back(task_id);
                    *changed = changed.saturating_add(1);
                }
                None => break,
            }
        }
        woken
    }

    /// 将任务从 `from` 队列迁移到 `to` 队列（同时更新超时目标）。
    fn move_tasks_between_queues(&mut self,
                                 from_wait_queue_id : WaitQueueId,
                                 to_wait_queue_id : WaitQueueId,
                                 count : usize,
                                 changed : &mut usize)
                                 -> VecDeque<(TaskId, WaitQueueId)> {
        let mut moved = VecDeque::new();
        for _ in 0..count {
            let Some(task_id) = self.wait_queue_mut(from_wait_queue_id)
                                    .pop_front()
            else {
                break;
            };
            self.update_wait_timeout_target(task_id,
                                            from_wait_queue_id,
                                            to_wait_queue_id);
            self.wait_queue_mut(to_wait_queue_id)
                .push_back(task_id);
            moved.push_back((task_id, from_wait_queue_id));
            *changed = changed.saturating_add(1);
        }
        moved
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

    fn remove_wait_timeout(&mut self, task_id : TaskId, target : TaskWaitTarget) {
        self.wait_timeouts
            .retain(|entry| entry.task_id != task_id || entry.wait_target != target);
    }

    /// 从任务登记的精确等待目标摘除它。调用方已经单独移除当前超时记录，
    /// 因而这里不再扫描 `wait_timeouts`。
    fn remove_task_from_wait_target(&mut self, task_id : TaskId, target : TaskWaitTarget) -> bool {
        match target {
            TaskWaitTarget::WaitQueue(wait_queue_id) => {
                take_task_id_by_id(self.wait_queue_mut(wait_queue_id),
                                   task_id)
            }
            TaskWaitTarget::ChildExit(parent_id) => {
                take_task_id_by_id(self.child_exit_wait_queue_mut(parent_id),
                                   task_id)
            }
            TaskWaitTarget::TaskExit(target_id) => {
                take_task_id_by_id(self.exit_wait_queue_mut(target_id),
                                   task_id)
            }
            TaskWaitTarget::Manual => take_task_id_by_id(&mut self.blocked_queue, task_id),
        }
    }

    /// 从退出等待队列中取出所有等 `task_id` 退出的 waiter，返回其 ID 列表。
    pub fn wake_all_waiters_for_task_exit(&mut self, task_id : TaskId) -> VecDeque<TaskId> {
        let mut woken = VecDeque::new();
        while let Some(waiter) = self.exit_wait_queue_mut(task_id)
                                     .pop_front()
        {
            woken.push_back(waiter);
        }
        woken
    }

    /// 从子退出等待队列中取出属于 `parent_id` 的所有 waiter，返回其 ID 列表。
    pub fn wake_child_exit_waiters(&mut self, parent_id : TaskId) -> VecDeque<TaskId> {
        let mut woken = VecDeque::new();
        while let Some(waiter) = self.child_exit_wait_queue_mut(parent_id)
                                     .pop_front()
        {
            woken.push_back(waiter);
        }
        woken
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_removes_task_from_explicit_wait_queue() {
        let mut queues = WaitQueues::new();
        let wait_queue_id = queues.allocate_wait_queue("test-timeout");
        let target = TaskWaitTarget::WaitQueue(wait_queue_id);
        queues.enqueue_wait_task(7, target);
        queues.enqueue_wait_timeout(7, target, 1);

        queues.tick();

        assert_eq!(queues.timeout_tasks()
                         .pop_front(),
                   Some((7, target)));
        assert_eq!(queues.wake_one_in_wait_queue(wait_queue_id),
                   None);
        assert!(queues.try_release_wait_queue(wait_queue_id));
    }

    #[test]
    fn requeued_timeout_is_removed_from_destination_queue() {
        let mut queues = WaitQueues::new();
        let from = queues.allocate_wait_queue("test-from");
        let to = queues.allocate_wait_queue("test-to");
        let from_target = TaskWaitTarget::WaitQueue(from);
        let to_target = TaskWaitTarget::WaitQueue(to);
        queues.enqueue_wait_task(11, from_target);
        queues.enqueue_wait_timeout(11, from_target, 1);

        let (_, moved, changed) = queues.requeue_wait_queue(from, to, 0, 1);
        assert_eq!(moved.front()
                        .map(|(task_id, _)| *task_id),
                   Some(11));
        assert_eq!(changed, 1);
        queues.tick();

        assert_eq!(queues.timeout_tasks()
                         .pop_front(),
                   Some((11, to_target)));
        assert_eq!(queues.wake_one_in_wait_queue(to), None);
    }
}
