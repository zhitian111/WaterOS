// 等待队列操作与唤醒后的 CPU 归属。
use super::*;
impl MultiClassScheduler {
    pub fn allocate_wait_queue(&mut self) -> WaitQueueId {
        self.wait_queues
            .allocate_wait_queue()
    }

    pub fn try_release_wait_queue(&mut self, wait_queue_id : WaitQueueId) -> bool {
        self.wait_queues
            .try_release_wait_queue(wait_queue_id)
    }

    pub fn wake_task(&mut self, task_id : TaskId) -> bool {
        if !self.wait_queues
                .wake_task(task_id) ||
           self.registry
               .state(task_id)
               .is_none()
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
           self.registry
               .state(task_id)
               .is_none()
        {
            return false;
        }
        self.registry
            .finish_wait(task_id, TaskWaitResult::Interrupted);
        self.activate_ready_task(task_id, ReadyPlacement::LastCpu);
        true
    }

    pub fn block_task_manual(&mut self, task_id : TaskId, cpu_id : CpuId) {
        if self.registry
               .state(task_id)
               .is_none()
        {
            return;
        }
        self.cpu_states[cpu_id.raw()].dequeue(task_id);
        self.registry
            .mark_blocking(task_id, TaskWaitTarget::Manual);
        self.wait_queues
            .block_task_manual(task_id);
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
        let task_id = self.wait_queues
                          .wake_one_in_wait_queue(wait_queue_id)?;
        if self.registry
               .state(task_id)
               .is_none()
        {
            return None;
        }
        self.registry
            .finish_wait(task_id, TaskWaitResult::Woken);
        self.activate_ready_task(task_id, ReadyPlacement::LastCpu);
        Some(task_id)
    }

    pub fn wake_all_in_wait_queue(&mut self, wait_queue_id : WaitQueueId) -> usize {
        let task_ids = self.wait_queues
                           .wake_all_in_wait_queue(wait_queue_id);
        let mut count = 0usize;
        for task_id in task_ids {
            if self.registry
                   .state(task_id)
                   .is_none()
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
    pub fn requeue_wait_queue(&mut self,
                              from_wait_queue_id : WaitQueueId,
                              to_wait_queue_id : WaitQueueId,
                              wake_count : usize,
                              requeue_count : usize)
                              -> usize {
        let (woken, moved, changed) = self.wait_queues
                                          .requeue_wait_queue(from_wait_queue_id,
                                                              to_wait_queue_id,
                                                              wake_count,
                                                              requeue_count);
        for task_id in woken {
            self.registry
                .finish_wait(task_id, TaskWaitResult::Woken);
            self.activate_ready_task(task_id, ReadyPlacement::LastCpu);
        }
        for (task_id, _) in moved {
            self.registry
                .mark_blocking(task_id,
                               TaskWaitTarget::WaitQueue(to_wait_queue_id));
        }
        changed
    }
}
