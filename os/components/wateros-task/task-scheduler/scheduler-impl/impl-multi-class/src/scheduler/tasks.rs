// 任务创建、fork、clone 与 exec 的调度器内部实现。
use super::*;
impl MultiClassScheduler {
    pub fn spawn_kernel_task(&mut self,
                             entry : KernelTaskEntry,
                             arg : usize,
                             cpu_id : CpuId)
                             -> TaskId {
        let current_task_id = self.cpu_states[cpu_id.raw()].current_task_id;
        let task_id = self.registry
                          .spawn_kernel_task(entry, arg, current_task_id);
        self.enqueue_ready_task(task_id);
        task_id
    }

    pub fn create_user_task_spec(&mut self, spec : UserTask, cpu_id : CpuId) -> TaskId {
        let current_task_id = self.cpu_states[cpu_id.raw()].current_task_id;
        self.registry
            .spawn_user_task_spec(spec, current_task_id)
    }

    pub fn spawn_user_task_spec(&mut self, spec : UserTask, cpu_id : CpuId) -> TaskId {
        let task_id = self.create_user_task_spec(spec, cpu_id);
        self.enqueue_ready_task(task_id);
        task_id
    }

    /// 为新建任务选择 online CPU 中就绪队列负载最小的一个。
    ///
    /// 遍历从 `next_placement_cpu` 开始，因此同样负载不会永远偏向 CPU 0。
    pub fn pick_cpu_for_new_task(&mut self, task_id : TaskId) -> CpuId {
        let affinity = self.registry
                           .get_affinity(task_id)
                           .expect("queued task must exist");
        let mut best_cpu = None;
        let mut min_load = usize::MAX;
        let cpu_count = self.cpu_states
                            .len();
        for offset in 0..cpu_count {
            let i = (self.next_placement_cpu + offset) % cpu_count;
            if !self.cpu_states[i].online || !affinity.contains(CpuId::from_raw(i)) {
                continue;
            }
            let load = self.cpu_load(CpuId::from_raw(i));
            if load < min_load {
                min_load = load;
                best_cpu = Some(CpuId::from_raw(i));
            }
        }
        let best_cpu = best_cpu.expect("cannot enqueue a task without an online CPU");
        self.next_placement_cpu = (best_cpu.raw() + 1) % cpu_count;
        best_cpu
    }


    pub fn create_fork_child(&mut self,
                             child_stack : usize,
                             new_aspace_ptr : usize,
                             new_satp : usize,
                             child_parent_id : TaskId,
                             cpu_id : CpuId)
                             -> Option<TaskId> {
        let source_task_id = self.cpu_states[cpu_id.raw()].current_task_id?;
        self.registry
            .fork_current(child_stack,
                          new_aspace_ptr,
                          new_satp,
                          source_task_id,
                          child_parent_id)
    }

    pub fn fork_current(&mut self,
                        child_stack : usize,
                        new_aspace_ptr : usize,
                        new_satp : usize,
                        cpu_id : CpuId)
                        -> Option<TaskId> {
        let child_id = self.create_fork_child(child_stack,
                                              new_aspace_ptr,
                                              new_satp,
                                              self.cpu_states[cpu_id.raw()].current_task_id?,
                                              cpu_id)?;
        self.enqueue_ready_task(child_id);
        Some(child_id)
    }

    pub fn create_clone_thread(&mut self,
                               child_stack : usize,
                               tls : usize,
                               set_tls : bool,
                               cpu_id : CpuId)
                               -> Option<TaskId> {
        let parent_id = self.cpu_states[cpu_id.raw()].current_task_id?;
        self.registry
            .clone_current_thread(child_stack, tls, set_tls, parent_id)
    }

    pub fn clone_current_thread(&mut self,
                                child_stack : usize,
                                tls : usize,
                                set_tls : bool,
                                cpu_id : CpuId)
                                -> Option<TaskId> {
        let child_id = self.create_clone_thread(child_stack, tls, set_tls, cpu_id)?;
        self.enqueue_ready_task(child_id);
        Some(child_id)
    }

    pub fn execve_current(&mut self,
                          entry_pc : usize,
                          sp : usize,
                          argc : usize,
                          argv : usize,
                          envp : usize,
                          satp : usize,
                          user_aspace_ptr : usize,
                          image_info : task_api::UserImageInfo,
                          stack_info : task_api::UserStack,
                          cpu_id : CpuId) {
        let current_id = self.cpu_states[cpu_id.raw()].current_task_id
                                                      .expect("execve requires a current task");
        self.registry
            .execve_current(entry_pc,
                            sp,
                            argc,
                            argv,
                            envp,
                            satp,
                            user_aspace_ptr,
                            image_info,
                            stack_info,
                            current_id);
    }
    pub fn current_task_id(&self, cpu_id : CpuId) -> Option<TaskId> {
        self.cpu_states[cpu_id.raw()].current_task_id
    }

    pub fn current_task_snapshot(&self, cpu_id : CpuId) -> Option<TaskSnapshot> {
        Some(self.registry
                 .task_snapshot(self.cpu_states[cpu_id.raw()].current_task_id?))
    }

    pub fn task_snapshot(&self, task_id : TaskId) -> TaskSnapshot {
        self.registry
            .task_snapshot(task_id)
    }

    pub fn has_child(&self, parent_id : TaskId) -> bool {
        self.registry
            .has_child(parent_id)
    }

    pub fn current_tick(&self) -> TaskTick {
        self.wait_queues
            .current_tick()
    }

    pub fn current_task_kernel_stack_top(&self, cpu_id : CpuId) -> Option<usize> {
        self.cpu_states[cpu_id.raw()].current_task_id
                                     .map(|task_id| {
                                         self.registry
                                             .task_kernel_stack_top(task_id)
                                     })
    }

    pub fn current_task_address_space_raw(&self, cpu_id : CpuId) -> usize {
        self.cpu_states[cpu_id.raw()].current_task_id
                                     .map(|task_id| {
                                         self.registry
                                             .current_task_address_space_raw(task_id)
                                     })
                                     .unwrap_or(0)
    }

    pub fn current_task_user_aspace_ptr(&self, cpu_id : CpuId) -> usize {
        self.cpu_states[cpu_id.raw()].current_task_id
                                     .map(|task_id| {
                                         self.registry
                                             .current_task_user_aspace_ptr(task_id)
                                     })
                                     .unwrap_or(0)
    }

    pub fn current_task_user_address_space_token(&self, cpu_id : CpuId) -> usize {
        self.current_task_address_space_raw(cpu_id)
    }

    pub fn current_task_trap_return_address_space_token(&self, cpu_id : CpuId) -> usize {
        self.cpu_states[cpu_id.raw()].current_task_id
                                     .map(|task_id| {
                                         self.registry
                                             .current_task_trap_return_address_space_token(task_id)
                                     })
                                     .unwrap_or(0)
    }

    pub fn begin_current_trap_frame_access(&mut self,
                                           trap_frame : TaskTrapFrame,
                                           cpu_id : CpuId)
                                           -> Option<*mut TaskTrapFrame> {
        let task_id = self.cpu_states[cpu_id.raw()].current_task_id?;
        self.registry
            .begin_trap_frame_access(trap_frame, task_id)
    }

    pub fn restore_current_trap_frame(&self,
                                      trap_frame : &mut TaskTrapFrame,
                                      cpu_id : CpuId)
                                      -> bool {
        let Some(task_id) = self.cpu_states[cpu_id.raw()].current_task_id else {
            return false;
        };
        self.registry
            .restore_trap_frame(trap_frame, task_id)
    }

    pub fn take_current_wait_result(&mut self, cpu_id : CpuId) -> TaskWaitResult {
        let task_id =
            self.cpu_states[cpu_id.raw()].current_task_id
                                         .expect("wait result can only be taken for a running \
                                                  task");
        self.registry
            .take_current_wait_result(task_id)
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
                        let target = self.pick_cpu_for_new_task(task_id);
                        self.enqueue_ready_by_cpu(task_id, target);
                        self.request_reschedule(target,
                                                RescheduleRequest::Ready(task_id));
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
                    self.request_reschedule(running_cpu, RescheduleRequest::Forced);
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
            self.cpu_states[running_cpu.raw()].current_nice = nice;
        }
        self.registry
            .set_nice(task_id, nice)
    }

    pub fn get_nice(&self, task_id : TaskId) -> Result<i8, SchedError> {
        let snap = self.registry
                       .task_snapshot(task_id);
        Ok(snap.nice)
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
