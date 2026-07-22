// 任务创建、fork、clone 与 exec 的调度器内部实现。

impl MultiClassScheduler {
    pub(super) fn spawn_kernel_task(&mut self,
                                    entry : KernelTaskEntry,
                                    arg : usize,
                                    cpu_id : CpuId)
                                    -> TaskId {
        let current_task_id = self.cpu_states[cpu_id.raw()].current_task_id;
        let task_id = self.global
                          .registry
                          .spawn_kernel_task(entry, arg, current_task_id);
        self.enqueue_ready_task(task_id);
        task_id
    }

    pub(super) fn create_user_task_spec(&mut self, spec : UserTask, cpu_id : CpuId) -> TaskId {
        let current_task_id = self.cpu_states[cpu_id.raw()].current_task_id;
        self.global
            .registry
            .spawn_user_task_spec(spec, current_task_id)
    }

    pub(super) fn spawn_user_task_spec(&mut self, spec : UserTask, cpu_id : CpuId) -> TaskId {
        let task_id = self.create_user_task_spec(spec, cpu_id);
        self.enqueue_ready_task(task_id);
        task_id
    }

    /// 为新建任务选择 online CPU 中就绪队列负载最小的一个。
    ///
    /// 遍历从 `next_placement_cpu` 开始，因此同样负载不会永远偏向 CPU 0。
    pub(super) fn pick_cpu_for_new_task(&mut self) -> CpuId {
        let mut best_cpu = None;
        let mut min_load = usize::MAX;
        let cpu_count = self.cpu_states
                            .len();
        for offset in 0..cpu_count {
            let i = (self.next_placement_cpu + offset) % cpu_count;
            if !self.cpu_states[i].online {
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


    pub(super) fn create_fork_child(&mut self,
                                    child_stack : usize,
                                    new_aspace_ptr : usize,
                                    new_satp : usize,
                                    cpu_id : CpuId)
                                    -> Option<TaskId> {
        let current_task_id = self.cpu_states[cpu_id.raw()].current_task_id?;
        self.global
            .registry
            .fork_current(child_stack,
                          new_aspace_ptr,
                          new_satp,
                          current_task_id)
    }

    pub(super) fn fork_current(&mut self,
                               child_stack : usize,
                               new_aspace_ptr : usize,
                               new_satp : usize,
                               cpu_id : CpuId)
                               -> Option<TaskId> {
        let child_id = self.create_fork_child(child_stack,
                                              new_aspace_ptr,
                                              new_satp,
                                              cpu_id)?;
        self.enqueue_ready_task(child_id);
        Some(child_id)
    }

    pub(super) fn create_clone_thread(&mut self,
                                      child_stack : usize,
                                      tls : usize,
                                      set_tls : bool,
                                      cpu_id : CpuId)
                                      -> Option<TaskId> {
        let parent_id = self.cpu_states[cpu_id.raw()].current_task_id?;
        self.global
            .registry
            .clone_current_thread(child_stack, tls, set_tls, parent_id)
    }

    pub(super) fn clone_current_thread(&mut self,
                                       child_stack : usize,
                                       tls : usize,
                                       set_tls : bool,
                                       cpu_id : CpuId)
                                       -> Option<TaskId> {
        let child_id = self.create_clone_thread(child_stack, tls, set_tls, cpu_id)?;
        self.enqueue_ready_task(child_id);
        Some(child_id)
    }

    pub(super) fn execve_current(&mut self,
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
        self.global
            .registry
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
    pub(super) fn current_task_id(&self, cpu_id : CpuId) -> Option<TaskId> {
        self.cpu_states[cpu_id.raw()].current_task_id
    }

    pub(super) fn current_task_snapshot(&self, cpu_id : CpuId) -> Option<TaskSnapshot> {
        Some(self.global
                 .registry
                 .task_snapshot(self.cpu_states[cpu_id.raw()].current_task_id?))
    }

    pub(super) fn task_snapshot(&self, task_id : TaskId) -> TaskSnapshot {
        self.global
            .registry
            .task_snapshot(task_id)
    }

    pub(super) fn has_child(&self, parent_id : TaskId) -> bool {
        self.global
            .registry
            .has_child(parent_id)
    }

    pub(super) fn current_tick(&self) -> TaskTick {
        self.global
            .wait_queues
            .current_tick()
    }

    pub(super) fn current_task_kernel_stack_top(&self, cpu_id : CpuId) -> Option<usize> {
        Some(self.global
                 .registry
                 .task_kernel_stack_top(self.cpu_states[cpu_id.raw()].current_task_id?))
    }

    pub(super) fn current_task_address_space_raw(&self, cpu_id : CpuId) -> usize {
        self.cpu_states[cpu_id.raw()].current_task_id
                                     .map(|id| {
                                         self.global
                                             .registry
                                             .current_task_address_space_raw(id)
                                     })
                                     .unwrap_or(0)
    }

    pub(super) fn current_task_user_aspace_ptr(&self, cpu_id : CpuId) -> usize {
        self.cpu_states[cpu_id.raw()].current_task_id
                                     .map(|id| {
                                         self.global
                                             .registry
                                             .current_task_user_aspace_ptr(id)
                                     })
                                     .unwrap_or(0)
    }

    pub(super) fn current_task_user_address_space_token(&self, cpu_id : CpuId) -> usize {
        self.current_task_address_space_raw(cpu_id)
    }

    pub(super) fn current_task_trap_return_address_space_token(&self, cpu_id : CpuId) -> usize {
        self.cpu_states[cpu_id.raw()].current_task_id
                                     .map(|id| {
                                         self.global
                                             .registry
                                             .current_task_trap_return_address_space_token(id)
                                     })
                                     .unwrap_or(0)
    }

    pub(super) fn begin_current_trap_frame_access(&mut self,
                                                  trap_frame : TaskTrapFrame,
                                                  cpu_id : CpuId)
                                                  -> Option<*mut TaskTrapFrame> {
        let task_id = self.cpu_states[cpu_id.raw()].current_task_id?;
        self.global
            .registry
            .begin_trap_frame_access(trap_frame, task_id)
    }

    pub(super) fn restore_current_trap_frame(&self,
                                             trap_frame : &mut TaskTrapFrame,
                                             cpu_id : CpuId)
                                             -> bool {
        let task_id = match self.cpu_states[cpu_id.raw()].current_task_id {
            Some(id) => id,
            None => return false,
        };
        self.global
            .registry
            .restore_trap_frame(trap_frame, task_id)
    }

    pub(super) fn take_current_wait_result(&mut self, cpu_id : CpuId) -> TaskWaitResult {
        let task_id =
            self.cpu_states[cpu_id.raw()].current_task_id
                                         .expect("wait result can only be taken for a running \
                                                  task");
        self.global
            .registry
            .take_current_wait_result(task_id)
    }
}
