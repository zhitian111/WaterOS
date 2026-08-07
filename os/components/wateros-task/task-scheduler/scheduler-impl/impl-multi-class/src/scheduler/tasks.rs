// 任务创建、fork、clone 与 exec 的调度器内部实现。
use super::*;
impl MultiClassScheduler {
    pub fn spawn_kernel_task(&mut self,
                             entry : KernelTaskEntry,
                             arg : usize,
                             cpu_id : CpuId)
                             -> TaskId {
        let current_task_id = self.cpu_states[cpu_id.raw()].current_task_id();
        let task_id = self.registry
                          .spawn_kernel_task(entry, arg, current_task_id);
        self.activate_ready_task(task_id, ReadyPlacement::LeastLoaded);
        task_id
    }

    pub fn create_user_task_spec(&mut self, spec : UserTask, cpu_id : CpuId) -> TaskId {
        let current_task_id = self.cpu_states[cpu_id.raw()].current_task_id();
        self.registry
            .spawn_user_task_spec(spec, current_task_id)
    }

    pub fn spawn_user_task_spec(&mut self, spec : UserTask, cpu_id : CpuId) -> TaskId {
        let task_id = self.create_user_task_spec(spec, cpu_id);
        self.activate_ready_task(task_id, ReadyPlacement::LeastLoaded);
        task_id
    }

    pub fn create_fork_child(&mut self,
                             child_stack : usize,
                             new_aspace_ptr : usize,
                             new_satp : usize,
                             child_parent_id : TaskId,
                             cpu_id : CpuId)
                             -> Option<TaskId> {
        let source_task_id = self.cpu_states[cpu_id.raw()].current_task_id()?;
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
                                              self.cpu_states[cpu_id.raw()].current_task_id()?,
                                              cpu_id)?;
        self.activate_ready_task(child_id, ReadyPlacement::LeastLoaded);
        Some(child_id)
    }

    pub fn create_clone_thread(&mut self,
                               child_stack : usize,
                               tls : usize,
                               set_tls : bool,
                               cpu_id : CpuId)
                               -> Option<TaskId> {
        let parent_id = self.cpu_states[cpu_id.raw()].current_task_id()?;
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
        self.activate_ready_task(child_id, ReadyPlacement::LeastLoaded);
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
        let current_id = self.cpu_states[cpu_id.raw()].current_task_id()
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
        let cpu = &mut self.cpu_states[cpu_id.raw()];
        let previous_aspace = cpu.current_aspace();
        if previous_aspace != user_aspace_ptr {
            mm_api::user_aspace_lifecycle::notify_aspace_cpu_leave(previous_aspace, cpu_id);
            mm_api::user_aspace_lifecycle::notify_aspace_cpu_enter(user_aspace_ptr, cpu_id);
            cpu.set_current_aspace(user_aspace_ptr);
        }
    }
    pub fn begin_current_trap_frame_access(&mut self,
                                           trap_frame : TaskTrapFrame,
                                           cpu_id : CpuId)
                                           -> Option<*mut TaskTrapFrame> {
        let task_id = self.cpu_states[cpu_id.raw()].current_task_id()?;
        self.registry
            .begin_trap_frame_access(trap_frame, task_id)
    }

    pub fn restore_current_trap_frame(&self,
                                      trap_frame : &mut TaskTrapFrame,
                                      cpu_id : CpuId)
                                      -> bool {
        let Some(task_id) = self.cpu_states[cpu_id.raw()].current_task_id() else {
            return false;
        };
        self.registry
            .restore_trap_frame(trap_frame, task_id)
    }

    pub fn take_current_wait_result(&mut self, cpu_id : CpuId) -> TaskWaitResult {
        let task_id =
            self.cpu_states[cpu_id.raw()].current_task_id()
                                         .expect("wait result can only be taken for a running \
                                                  task");
        self.registry
            .take_current_wait_result(task_id)
    }
    pub fn kill_task(&mut self, task_id : TaskId, exit_code : TaskExitCode) -> bool {
        if self.registry
               .is_idle_task(task_id)
        {
            return false;
        }
        if self.registry
               .state(task_id)
               .is_none()
        {
            return false;
        }
        if matches!(self.registry
                        .state(task_id),
                    Some(TaskState::Exited(_)))
        {
            return true;
        }
        if self.cpu_states
               .iter()
               .any(|cpu| cpu.current_task_id() == Some(task_id))
        {
            return false;
        }
        self.dequeue_from_all_cpus(task_id);
        self.wait_queues
            .kill_task(task_id);
        self.registry
            .mark_exited(task_id, exit_code);
        true
    }

    pub fn discard_unstarted_task(&mut self, task_id : TaskId) {
        self.dequeue_from_all_cpus(task_id);
        self.wait_queues
            .detach_task_from_run_queues(task_id);
        self.registry
            .discard_task(task_id);
    }

    pub fn reap_exited_task(&mut self, task_id : TaskId) -> Option<ExitedTask> {
        self.wait_queues
            .reap_exited_task(&mut self.registry, task_id)
    }

    pub fn reap_one_exited_task(&mut self) -> Option<ExitedTask> {
        self.wait_queues
            .reap_one_exited_task(&mut self.registry)
    }

    pub fn reap_one_exited_child(&mut self, parent_id : TaskId) -> Option<ExitedTask> {
        let task_id = self.registry
                          .find_exited_child(parent_id)?;
        self.reap_exited_task(task_id)
    }
}
