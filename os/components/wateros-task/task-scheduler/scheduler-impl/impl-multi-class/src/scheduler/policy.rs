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
        let ready_cpu = old_snap.ready_cpu_id;
        let running_cpu = old_snap.running_cpu_id;

        // `current_policy` 等是 CPU 本地热路径缓存。修改正在运行任务的
        // SCHED_FIFO/RR 属性前，先回写旧统计；随后必须用新快照更新该 CPU，
        // 否则 tick 和抢占仍会按旧 SCHED_OTHER 属性执行。
        if let Some(running_cpu) = running_cpu {
            self.sync_current_to_registry(running_cpu);
        }

        self.dequeue_from_all_cpus(task_id);
        if !self.registry
                .set_task_sched(task_id, policy, priority)
        {
            return Err(SchedError::NoSuchTask);
        }
        let new_snap = self.registry
                           .task_snapshot(task_id);
        let mut reschedule_local = false;
        if was_ready {
            let target_cpu = ready_cpu.unwrap_or(cpu_id);
            self.enqueue_ready_by_cpu(task_id, target_cpu);
            if self.cpu_states[target_cpu.raw()].cpu_should_reschedule()
                                                .is_some()
            {
                if target_cpu == cpu_id {
                    reschedule_local = true;
                } else {
                    self.request_reschedule(target_cpu);
                }
            }
        }

        if let Some(running_cpu) = running_cpu {
            self.cpu_states[running_cpu.raw()].set_current_task(&new_snap);
            if self.cpu_states[running_cpu.raw()].cpu_should_reschedule()
                                                 .is_some()
            {
                if running_cpu == cpu_id {
                    reschedule_local = true;
                } else {
                    self.request_reschedule(running_cpu);
                }
            }
        }
        Ok(reschedule_local)
    }
}
