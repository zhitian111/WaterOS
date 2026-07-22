// 等待队列操作与唤醒后的 CPU 归属。

impl MultiClassScheduler {
    pub(super) fn allocate_wait_queue(&mut self) -> WaitQueueId {
        self.global
            .wait_queues
            .allocate_wait_queue()
    }

    pub(super) fn try_release_wait_queue(&mut self, wait_queue_id : WaitQueueId) -> bool {
        self.global
            .wait_queues
            .try_release_wait_queue(wait_queue_id)
    }

    pub(super) fn wake_task(&mut self, task_id : TaskId) -> bool {
        if !self.global
                .wait_queues
                .wake_task(task_id) ||
           self.global
               .registry
               .state(task_id)
               .is_none()
        {
            return false;
        }
        self.global
            .registry
            .finish_wait(task_id, TaskWaitResult::Woken);
        self.enqueue_woken_task(task_id);
        true
    }
    pub(super) fn interrupt_task(&mut self, task_id : TaskId) -> bool {
        if !self.global
                .wait_queues
                .interrupt_task(task_id) ||
           self.global
               .registry
               .state(task_id)
               .is_none()
        {
            return false;
        }
        self.global
            .registry
            .finish_wait(task_id, TaskWaitResult::Interrupted);
        self.enqueue_woken_task(task_id);
        true
    }

    pub(super) fn block_task_manual(&mut self, task_id : TaskId, cpu_id : CpuId) {
        if self.global
               .registry
               .state(task_id)
               .is_none()
        {
            return;
        }
        self.detach_from_run_queues(task_id, cpu_id);
        self.global
            .registry
            .mark_blocking(task_id, TaskWaitTarget::Manual);
        self.global
            .wait_queues
            .block_task_manual(task_id);
    }

    pub(super) fn wake_child_exit_waiters(&mut self, parent_id : TaskId) {
        let waiters = self.global
                          .wait_queues
                          .wake_child_exit_waiters(parent_id);
        for task_id in waiters {
            self.global
                .registry
                .finish_wait(task_id, TaskWaitResult::Woken);
            self.enqueue_woken_task(task_id);
        }
    }

    pub(super) fn wake_one_in_wait_queue(&mut self, wait_queue_id : WaitQueueId) -> Option<TaskId> {
        let task_id = self.global
                          .wait_queues
                          .wake_one_in_wait_queue(wait_queue_id)?;
        if self.global
               .registry
               .state(task_id)
               .is_none()
        {
            return None;
        }
        self.global
            .registry
            .finish_wait(task_id, TaskWaitResult::Woken);
        self.enqueue_woken_task(task_id);
        Some(task_id)
    }

    pub(super) fn wake_all_in_wait_queue(&mut self, wait_queue_id : WaitQueueId) -> usize {
        let task_ids = self.global
                           .wait_queues
                           .wake_all_in_wait_queue(wait_queue_id);
        let mut count = 0usize;
        for task_id in task_ids {
            if self.global
                   .registry
                   .state(task_id)
                   .is_none()
            {
                continue;
            }
            self.global
                .registry
                .finish_wait(task_id, TaskWaitResult::Woken);
            self.enqueue_woken_task(task_id);
            count = count.saturating_add(1);
        }
        count
    }
    pub(super) fn requeue_wait_queue(&mut self,
                                     from_wait_queue_id : WaitQueueId,
                                     to_wait_queue_id : WaitQueueId,
                                     wake_count : usize,
                                     requeue_count : usize)
                                     -> usize {
        let (woken, moved, changed) = self.global
                                          .wait_queues
                                          .requeue_wait_queue(from_wait_queue_id,
                                                              to_wait_queue_id,
                                                              wake_count,
                                                              requeue_count);
        for task_id in woken {
            self.global
                .registry
                .finish_wait(task_id, TaskWaitResult::Woken);
            self.enqueue_woken_task(task_id);
        }
        for (task_id, _) in moved {
            self.global
                .registry
                .mark_blocking(task_id,
                               TaskWaitTarget::WaitQueue(to_wait_queue_id));
        }
        changed
    }
}
