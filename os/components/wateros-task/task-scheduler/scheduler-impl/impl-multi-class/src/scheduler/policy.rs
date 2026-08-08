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
        match old_snap.state {
            // 运行态的 vruntime/ticks 在 CPU 热路径 cache 中，必须先回写，
            // 再修改 TCB 中的 policy。
            TaskState::Running => {
                let running_cpu = old_snap.running_cpu_id
                                          .expect("running task must have a CPU owner");
                self.sync_current_to_registry(running_cpu);
            }
            TaskState::Ready => {}
            TaskState::Blocking(_) | TaskState::Sleeping { .. } => {}
            TaskState::Exited(_) => {
                return Err(SchedError::NoSuchTask);
            }
        }

        // 无论旧 policy 属于哪类，都先清理旧 runqueue，随后按新 policy 重建归属。
        self.dequeue_from_all_cpus(task_id);
        if !self.registry
                .set_task_sched(task_id, policy, priority)
        {
            return Err(SchedError::NoSuchTask);
        }
        let new_snap = self.registry
                           .task_snapshot(task_id);

        // TCB 更新后的恢复路径只取决于旧 state：Ready 重新进入新 policy 的
        // runqueue；Running 保持运行态，只刷新所属 CPU cache。
        let reschedule_local = match old_snap.state {
            TaskState::Ready => {
                let target_cpu = old_snap.ready_cpu_id
                                         .unwrap_or(cpu_id);
                self.enqueue_ready_on_cpu(task_id, target_cpu);
                if !self.cpu_states[target_cpu.raw()]
                       .cpu_should_reschedule(RescheduleCause::Ready(new_snap.policy))
                {
                    false
                } else if target_cpu == cpu_id {
                    true
                } else {
                    // 上面已经完成统一的 cpu_should_reschedule 判断。
                    self.mark_need_resched(target_cpu);
                    false
                }
            }
            TaskState::Running => {
                let running_cpu = old_snap.running_cpu_id
                                          .expect("running task must have a CPU owner");
                self.cpu_states[running_cpu.raw()].set_current_task(&new_snap);
                if !self.cpu_states[running_cpu.raw()].cpu_should_reschedule(RescheduleCause::Tick)
                {
                    false
                } else if running_cpu == cpu_id {
                    true
                } else {
                    // 上面已经完成统一的 cpu_should_reschedule 判断。
                    self.mark_need_resched(running_cpu);
                    false
                }
            }
            TaskState::Blocking(_) | TaskState::Sleeping { .. } => false,
            TaskState::Exited(_) => unreachable!("exited state rejected before TCB update"),
        };
        Ok(reschedule_local)
    }
}
