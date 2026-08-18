//! 等待队列操作与唤醒后的 CPU 归属。
use super::*;
use crate::WaitQueueRequeueResult;
impl MultiClassScheduler {
    pub fn allocate_wait_queue(&mut self, name : &'static str) -> WaitQueueId {
        self.wait_queues
            .allocate_wait_queue(name)
    }

    pub fn wait_queue_name(&self, wait_queue_id : WaitQueueId) -> Option<&'static str> {
        self.wait_queues
            .wait_queue_name(wait_queue_id)
    }

    pub fn try_release_wait_queue(&mut self, wait_queue_id : WaitQueueId) -> bool {
        self.wait_queues
            .try_release_wait_queue(wait_queue_id)
    }

    pub fn wake_task(&mut self, task_id : TaskId) -> bool {
        if !self.wait_queues
                .wake_task(task_id) ||
           !matches!(self.registry
                         .state(task_id),
                     Some(TaskState::Blocking(_)) | Some(TaskState::Sleeping { .. }))
        {
            return false;
        }
        self.registry
            .finish_wait(task_id, TaskWaitResult::Woken);
        self.activate_ready_task(task_id, ReadyPlacement::LastCpu);
        true
    }
    pub fn interrupt_task(&mut self, task_id : TaskId) -> bool {
        if !self.wait_queues
                .interrupt_task(task_id) ||
           !matches!(self.registry
                         .state(task_id),
                     Some(TaskState::Blocking(_)) | Some(TaskState::Sleeping { .. }))
        {
            return false;
        }
        self.registry
            .finish_wait(task_id, TaskWaitResult::Interrupted);
        self.activate_ready_task(task_id, ReadyPlacement::LastCpu);
        true
    }

    pub fn wake_child_exit_waiters(&mut self, parent_id : TaskId) {
        let waiters = self.wait_queues
                          .wake_child_exit_waiters(parent_id);
        for task_id in waiters {
            self.registry
                .finish_wait(task_id, TaskWaitResult::Woken);
            self.activate_ready_task(task_id, ReadyPlacement::LastCpu);
        }
    }

    pub fn wake_one_in_wait_queue(&mut self, wait_queue_id : WaitQueueId) -> Option<TaskId> {
        let target = TaskWaitTarget::WaitQueue(wait_queue_id);
        while let Some(task_id) = self.wait_queues
                                      .wake_one_in_wait_queue(wait_queue_id)
        {
            if self.registry
                   .state(task_id) !=
               Some(TaskState::Blocking(target))
            {
                // 防御性丢弃旧版本或异常路径遗留的陈旧队列项，绝不能重新
                // 激活一个已经 Ready/Running 的任务。
                continue;
            }
            self.registry
                .finish_wait(task_id, TaskWaitResult::Woken);
            self.activate_ready_task(task_id, ReadyPlacement::LastCpu);
            return Some(task_id);
        }
        None
    }

    pub fn wake_all_in_wait_queue(&mut self, wait_queue_id : WaitQueueId) -> usize {
        let task_ids = self.wait_queues
                           .wake_all_in_wait_queue(wait_queue_id);
        let target = TaskWaitTarget::WaitQueue(wait_queue_id);
        let mut count = 0usize;
        for task_id in task_ids {
            if self.registry
                   .state(task_id) !=
               Some(TaskState::Blocking(target))
            {
                continue;
            }
            self.registry
                .finish_wait(task_id, TaskWaitResult::Woken);
            self.activate_ready_task(task_id, ReadyPlacement::LastCpu);
            count = count.saturating_add(1);
        }
        count
    }
    pub fn requeue_wait_queue_detailed(&mut self,
                                       from_wait_queue_id : WaitQueueId,
                                       to_wait_queue_id : WaitQueueId,
                                       wake_count : usize,
                                       requeue_count : usize)
                                       -> WaitQueueRequeueResult {
        let (woken, moved, _) = self.wait_queues
                                    .requeue_wait_queue(from_wait_queue_id,
                                                        to_wait_queue_id,
                                                        wake_count,
                                                        requeue_count);
        let from_target = TaskWaitTarget::WaitQueue(from_wait_queue_id);
        let to_target = TaskWaitTarget::WaitQueue(to_wait_queue_id);
        let mut result = WaitQueueRequeueResult::default();
        for task_id in woken {
            if self.registry
                   .state(task_id) !=
               Some(TaskState::Blocking(from_target))
            {
                continue;
            }
            self.registry
                .finish_wait(task_id, TaskWaitResult::Woken);
            self.activate_ready_task(task_id, ReadyPlacement::LastCpu);
            result.woken.push(task_id);
        }
        for (task_id, _) in moved {
            if self.registry
                   .state(task_id) !=
               Some(TaskState::Blocking(from_target))
            {
                // `WaitQueues` 已把此项移到目标队列；若它不是合法 waiter，
                // 立即摘除，避免目标队列继续保存陈旧 TaskId。
                let _ = self.wait_queues
                            .wake_task(task_id);
                continue;
            }
            self.registry
                .mark_blocking(task_id, to_target);
            result.moved.push(task_id);
        }
        result
    }

    pub fn requeue_wait_queue(&mut self,
                              from_wait_queue_id : WaitQueueId,
                              to_wait_queue_id : WaitQueueId,
                              wake_count : usize,
                              requeue_count : usize)
                              -> usize {
        self.requeue_wait_queue_detailed(from_wait_queue_id,
                                         to_wait_queue_id,
                                         wake_count,
                                         requeue_count)
            .changed()
    }

    pub fn requeue_wait_queue_while(&mut self,
                                    from_wait_queue_id : WaitQueueId,
                                    to_wait_queue_id : WaitQueueId,
                                    wake_count : usize,
                                    requeue_count : usize,
                                    condition : impl FnOnce() -> bool)
                                    -> Option<usize> {
        if !condition() {
            return None;
        }
        Some(self.requeue_wait_queue(from_wait_queue_id,
                                     to_wait_queue_id,
                                     wake_count,
                                     requeue_count))
    }

    pub fn requeue_wait_queue_detailed_while(
        &mut self,
        from_wait_queue_id : WaitQueueId,
        to_wait_queue_id : WaitQueueId,
        wake_count : usize,
        requeue_count : usize,
        condition : impl FnOnce() -> bool)
        -> Option<WaitQueueRequeueResult> {
        if !condition() {
            return None;
        }
        Some(self.requeue_wait_queue_detailed(from_wait_queue_id,
                                              to_wait_queue_id,
                                              wake_count,
                                              requeue_count))
    }
}
