//! 调度策略修改与抢占比较。
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

    pub fn set_affinity(&mut self, task_id : TaskId, mask : CpuMask) -> Result<(), SchedError> {
        if mask.bits() & !CpuMask::ALL.bits() != 0 {
            return Err(SchedError::InvalidArg);
        }
        if mask.bits() &
           self.online_cpu_mask()
               .bits() ==
           0
        {
            return Err(SchedError::InvalidArg);
        }

        let state = self.registry
                        .state(task_id)
                        .ok_or(SchedError::NoSuchTask)?;
        if matches!(state, TaskState::Exited(_)) {
            return Err(SchedError::NoSuchTask);
        }
        self.registry
            .set_affinity(task_id, mask)?;

        match state {
            TaskState::Ready => {
                // create_* 会先登记一个 Ready TCB、稍后才入 runqueue。此时
                // `ready_cpu_id` 为 None；只保存 affinity，首次入队会按新 mask
                // 选核，不能把它当作调度器不变量而 panic。
                if let Some(ready_cpu) = self.registry
                                             .ready_cpu_id(task_id)
                {
                    if !mask.contains(ready_cpu) {
                        self.activate_ready_task(task_id, ReadyPlacement::LeastLoaded);
                    }
                }
            }
            TaskState::Running => {
                let running_cpu = self.registry
                                      .running_cpu_id(task_id)
                                      .expect("running task must have a CPU owner");
                if !mask.contains(running_cpu) {
                    // 不从远端 CPU 修改运行现场。由目标 CPU 在收到 IPI 后进入
                    // Reschedule 路径，把当前任务重新入队到允许的 CPU。
                    self.request_reschedule(running_cpu, RescheduleCause::Forced);
                }
            }
            TaskState::Blocking(_) | TaskState::Sleeping { .. } => {}
            TaskState::Exited(_) => unreachable!(),
        }
        Ok(())
    }
    pub fn get_affinity(&self, task_id : TaskId) -> Result<CpuMask, SchedError> {
        self.registry
            .get_affinity(task_id)
    }

    /// 更新 TCB 中的 nice；运行中的任务还必须同步其所属 CPU 的热路径 cache。
    pub fn set_nice(&mut self, task_id : TaskId, nice : i8) -> Result<(), SchedError> {
        let state = self.registry
                        .state(task_id)
                        .ok_or(SchedError::NoSuchTask)?;
        if matches!(state, TaskState::Exited(_)) {
            return Err(SchedError::NoSuchTask);
        }
        if let Some(running_cpu) = self.registry
                                       .running_cpu_id(task_id)
        {
            self.cpu_states[running_cpu.raw()].set_current_nice(nice);
        }
        self.registry
            .set_nice(task_id, nice)
    }

    pub fn get_nice(&self, task_id : TaskId) -> Result<i8, SchedError> {
        let snap = self.registry
                       .task_snapshot(task_id);
        Ok(snap.nice)
    }

    /// 设置线程级 I/O 优先级；块层可通过任务快照消费该属性。
    pub fn set_io_priority(&mut self,
                           task_id : TaskId,
                           io_priority : u16)
                           -> Result<(), SchedError> {
        let state = self.registry
                        .state(task_id)
                        .ok_or(SchedError::NoSuchTask)?;
        if matches!(state, TaskState::Exited(_)) {
            return Err(SchedError::NoSuchTask);
        }
        self.registry
            .set_io_priority(task_id, io_priority)
    }

    pub fn get_io_priority(&self, task_id : TaskId) -> Result<u16, SchedError> {
        Ok(self.registry
               .task_snapshot(task_id)
               .io_priority)
    }
    pub fn priority(&self, task_id : TaskId) -> Result<Priority, SchedError> {
        let snap = self.registry
                       .task_snapshot(task_id);
        Ok(snap.priority)
    }
    pub fn policy(&self, task_id : TaskId) -> Result<SchedPolicy, SchedError> {
        let snap = self.registry
                       .task_snapshot(task_id);
        Ok(snap.policy)
    }
}
