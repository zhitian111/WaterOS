// 调度策略修改与抢占比较。

impl MultiClassScheduler {
    pub(super) fn apply_sched_policy_change(&mut self,
                                            task_id : TaskId,
                                            policy : SchedPolicy,
                                            param : SchedParam,
                                            cpu_id : CpuId)
                                            -> Result<SchedPolicyChangeAction, SchedError> {
        if !self.global
                .registry
                .is_schedulable(task_id)
        {
            return Err(SchedError::NoSuchTask);
        }
        let old_snap = self.global
                           .registry
                           .task_snapshot(task_id);
        let was_ready = old_snap.state == TaskState::Ready;

        self.detach_from_all_cpus(task_id);
        if !self.global
                .registry
                .set_task_sched(task_id, policy, param.priority)
        {
            return Err(SchedError::NoSuchTask);
        }
        if was_ready {
            self.enqueue_ready_by_cpu(task_id, cpu_id);
        }
        // 如果当前 CPU 上有任务在运行，判断是否需要抢占。
        if let Some(current_id) = self.cpu_states[cpu_id.raw()].current_task_id {
            if current_id != task_id {
                let new = self.global
                              .registry
                              .task_snapshot(task_id);
                let current = self.global
                                  .registry
                                  .task_snapshot(current_id);
                if Self::cmp_priority(new.sched_policy,
                                      new.sched_priority,
                                      current.sched_policy,
                                      current.sched_priority)
                {
                    return Ok(SchedPolicyChangeAction::RescheduleNow);
                }
            }
        }
        Ok(SchedPolicyChangeAction::NoReschedule)
    }

    fn cmp_priority(challenger_policy : SchedPolicy,
                    challenger_priority : i32,
                    runner_policy : SchedPolicy,
                    runner_priority : i32)
                    -> bool {
        let challenger_class = match challenger_policy {
            SchedPolicy::Other => 0u8,
            SchedPolicy::Fifo | SchedPolicy::Rr => 1u8,
        };
        let runner_class = match runner_policy {
            SchedPolicy::Other => 0u8,
            SchedPolicy::Fifo | SchedPolicy::Rr => 1u8,
        };
        challenger_class > runner_class ||
        (challenger_class == runner_class && challenger_priority > runner_priority)
    }
}
