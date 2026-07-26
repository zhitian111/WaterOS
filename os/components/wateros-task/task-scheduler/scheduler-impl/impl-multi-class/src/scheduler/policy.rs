// 调度策略修改与抢占比较。
use super::*;
impl MultiClassScheduler {
    pub fn apply_sched_policy_change(&mut self,
                                     task_id : TaskId,
                                     policy : SchedPolicy,
                                     priority : Priority,
                                     cpu_id : CpuId)
                                     -> Result<bool, SchedError> {
        let old_snap = self.registry
                           .task_snapshot(task_id);
        if !matches!(old_snap.state,
                     TaskState::Ready | TaskState::Running)
        {
            return Err(SchedError::NoSuchTask);
        }
        let was_ready = old_snap.state == TaskState::Ready;

        self.dequeue_from_all_cpus(task_id);
        if !self.registry
                .set_task_sched(task_id, policy, priority)
        {
            return Err(SchedError::NoSuchTask);
        }
        if was_ready {
            self.enqueue_ready_by_cpu(task_id, cpu_id);
        }
        // 如果当前 CPU 上有任务在运行，判断是否需要抢占。
        if let Some(current_id) = self.cpu_states[cpu_id.raw()].current_task_id {
            if current_id != task_id && self.cpu_states[cpu_id.raw()].ready_task_should_preempt() {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
