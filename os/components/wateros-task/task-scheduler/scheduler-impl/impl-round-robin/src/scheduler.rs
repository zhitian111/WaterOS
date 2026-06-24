//! [`RoundRobinScheduler`]：`SCHED_OTHER` 轮转 + 共享 registry/等待队列。

use api_v0::{
    QueueTarget, ReadyQueue, ReadyTaskSink, SchedPolicyChangeAction, SwitchScheduler, TaskRegistry,
    WaitQueues,
};
use arch::task::ActiveArchTaskContext as TaskContext;
use task_api::{
    ExitedTask, KernelTaskEntry, SchedError, SchedParam, SchedPolicy, TaskBlockReason, TaskExitCode,
    TaskId, TaskSnapshot, TaskTick, TaskWaitHandle, TaskWaitResult, UserTask, WaitQueueId,
    IDLE_TASK_ID,
};

use crate::queues::OtherReadyQueue;
use crate::{SwitchPair, TaskTrapFrame};

use api_v0::ScheduleReason;

use config::task::MAX_TICKS_PER_TASK;

pub(super) struct RoundRobinScheduler {
    registry : TaskRegistry,
    wait : WaitQueues,
    other_ready : OtherReadyQueue,
    current_task_ticks : u64,
}

impl RoundRobinScheduler {
    pub(super) fn new() -> Self {
        Self { registry : TaskRegistry::new(),
               wait : WaitQueues::new(),
               other_ready : OtherReadyQueue::new(),
               current_task_ticks : 0 }
    }

    pub(super) fn init(&mut self) {
        self.registry.init();
        self.wait.init();
        self.other_ready.init();
        self.current_task_ticks = 0;
    }

    pub(super) fn apply_sched_policy_change(&mut self,
                                            task_id : TaskId,
                                            policy : SchedPolicy,
                                            param : SchedParam)
                                            -> Result<SchedPolicyChangeAction, SchedError>
    {
        if self.registry.task_snapshot(task_id).is_none() {
            return Err(SchedError::NoSuchTask);
        }
        if !self.registry.set_task_sched(task_id, policy, param.priority) {
            return Err(SchedError::NoSuchTask);
        }
        Ok(SchedPolicyChangeAction::NoReschedule)
    }

    pub(super) fn spawn_kernel_task(&mut self, entry : KernelTaskEntry, arg : usize) -> TaskId {
        let task_id = self.registry
                          .spawn_kernel_task(entry, arg);
        self.other_ready
            .enqueue_ready_task(task_id);
        log::debug!("[task-scheduler] spawned task {}",
                    task_id);
        task_id
    }

    pub(super) fn spawn_user_task_spec(&mut self, spec : UserTask) -> TaskId {
        let task_id = self.registry
                          .spawn_user_task_spec(spec);
        self.other_ready
            .enqueue_ready_task(task_id);
        log::debug!("[task-scheduler] spawned user task {}",
                    task_id);
        task_id
    }

    pub(super) fn allocate_wait_queue(&mut self) -> WaitQueueId {
        self.wait
            .allocate_wait_queue()
    }

    pub(super) fn try_release_wait_queue(&mut self, wait_queue_id : WaitQueueId) -> bool {
        self.wait
            .try_release_wait_queue(wait_queue_id)
    }

    pub(super) fn prepare_first_switch(&mut self) -> SwitchPair {
        let ready = self.other_ready.ready_queue_mut();
        self.wait
            .promote_sleeping_tasks(&mut self.registry, ready);
        self.wait
            .promote_wait_timeouts(&mut self.registry, ready);
        let next_task_id = self.other_ready
                               .pick_next_runnable_task_id(&self.registry)
                               .unwrap_or(IDLE_TASK_ID);
        self.current_task_ticks = 0;
        self.registry
            .first_switch_to(next_task_id)
    }

    fn finish_schedule_switch(&mut self,
                              current_task_id : TaskId,
                              current_ptr : *mut TaskContext,
                              is_exit : bool)
                              -> Option<SwitchPair>
    {
        let next_task_id = self.other_ready
                               .pick_next_runnable_task_id(&self.registry)
                               .unwrap_or(IDLE_TASK_ID);
        if next_task_id == current_task_id {
            if is_exit {
                if !self.registry
                        .is_idle(current_task_id)
                {
                    let next_ptr = self.registry
                                       .mark_running_and_set_current(IDLE_TASK_ID);
                    return Some((current_ptr, next_ptr));
                }
                panic!("exit_current: no runnable task after exit");
            }
            let _ = self.registry
                        .mark_running_and_set_current(next_task_id);
            return None;
        }
        let next_ptr = self.registry
                           .mark_running_and_set_current(next_task_id);
        Some((current_ptr, next_ptr))
    }

    pub(super) fn schedule(&mut self, reason : ScheduleReason) -> Option<SwitchPair> {
        match reason {
            ScheduleReason::Tick => {
                self.wait
                    .on_tick();
                self.registry
                    .account_tick_for_current();
                self.current_task_ticks = self.current_task_ticks
                                              .saturating_add(1);
                if self.current_task_ticks < MAX_TICKS_PER_TASK {
                    let ready = self.other_ready.ready_queue_mut();
                    self.wait
                        .promote_sleeping_tasks(&mut self.registry, ready);
                    self.wait
                        .promote_wait_timeouts(&mut self.registry, ready);
                    return None;
                }
                self.current_task_ticks = 0;
            }
            ScheduleReason::Sleep(ticks) if ticks == 0 => {
                return self.schedule(ScheduleReason::Yield);
            }
            _ => {
                self.current_task_ticks = 0;
            }
        }

        {
            let ready = self.other_ready.ready_queue_mut();
            self.wait
                .promote_sleeping_tasks(&mut self.registry, ready);
            self.wait
                .promote_wait_timeouts(&mut self.registry, ready);
        }

        let (current_task_id, current_ptr) = self.registry
                                                 .take_current_switch_out()?;
        if matches!(reason, ScheduleReason::Sleep(_)) {
            self.registry
                .clear_wait_result(current_task_id);
        }

        if self.registry
               .is_idle(current_task_id)
        {
            let next_task_id = self.other_ready
                                   .pick_next_runnable_task_id(&self.registry)
                                   .unwrap_or(IDLE_TASK_ID);
            if next_task_id == current_task_id {
                let _ = self.registry
                            .mark_running_and_set_current(next_task_id);
                return None;
            }
            let next_ptr = self.registry
                               .mark_running_and_set_current(next_task_id);
            return Some((current_ptr, next_ptr));
        }

        let is_exit = matches!(reason, ScheduleReason::Exit(_));
        let queue_target = match reason {
            ScheduleReason::StartFirst => QueueTarget::Ready,
            ScheduleReason::Yield | ScheduleReason::Tick => QueueTarget::Ready,
            ScheduleReason::Block(block_reason) => QueueTarget::Blocked(block_reason),
            ScheduleReason::Sleep(ticks) => {
                let wake_tick = self.wait
                                    .current_tick()
                                    .saturating_add(ticks.max(1));
                QueueTarget::Sleeping(wake_tick)
            }
            ScheduleReason::Exit(exit_code) => QueueTarget::Exited(exit_code),
        };

        if !matches!(queue_target, QueueTarget::Ready) {
            self.other_ready
                .detach_task(current_task_id);
        }
        {
            let ready = self.other_ready.ready_queue_mut();
            self.wait
                .enqueue_task(&mut self.registry,
                              current_task_id,
                              queue_target,
                              ready);
        }

        self.finish_schedule_switch(current_task_id, current_ptr, is_exit)
    }

    pub(super) fn schedule_wait(&mut self,
                                wait_handle : TaskWaitHandle,
                                timeout_ticks : Option<TaskTick>)
                                -> Option<SwitchPair>
    {
        self.current_task_ticks = 0;

        {
            let ready = self.other_ready.ready_queue_mut();
            self.wait
                .promote_sleeping_tasks(&mut self.registry, ready);
            self.wait
                .promote_wait_timeouts(&mut self.registry, ready);
        }

        if self.registry
               .wait_target_ready(wait_handle)
        {
            if let Some(current_task_id) = self.registry
                                               .current_task_id()
            {
                self.registry
                    .finish_wait(current_task_id, TaskWaitResult::Woken);
            }
            return None;
        }

        let (current_task_id, current_ptr) = self.registry
                                                 .take_current_switch_out()?;
        self.registry
            .clear_wait_result(current_task_id);
        self.other_ready
            .detach_task(current_task_id);
        {
            let ready = self.other_ready.ready_queue_mut();
            self.wait
                .enqueue_task(&mut self.registry,
                              current_task_id,
                              QueueTarget::Blocked(TaskBlockReason::Wait(wait_handle)),
                              ready);
        }
        if let Some(timeout_ticks) = timeout_ticks {
            let wake_tick = self.wait
                                .current_tick()
                                .saturating_add(timeout_ticks.max(1));
            self.wait
                .enqueue_wait_timeout(current_task_id, wait_handle, wake_tick);
        }

        let next_task_id = self.other_ready
                               .pick_next_runnable_task_id(&self.registry)
                               .unwrap_or(IDLE_TASK_ID);
        let next_ptr = self.registry
                           .mark_running_and_set_current(next_task_id);
        Some((current_ptr, next_ptr))
    }

    pub(super) fn wake_task(&mut self, task_id : TaskId) -> bool {
        self.wait
            .wake_task(&mut self.registry,
                       task_id,
                       self.other_ready.ready_queue_mut())
    }

    pub(super) fn interrupt_task(&mut self, task_id : TaskId) -> bool {
        self.wait
            .interrupt_task(&mut self.registry,
                            task_id,
                            self.other_ready.ready_queue_mut())
    }

    pub(super) fn kill_task(&mut self, task_id : TaskId, exit_code : TaskExitCode) -> bool {
        self.other_ready
            .detach_task(task_id);
        self.wait
            .kill_task(&mut self.registry,
                       task_id,
                       exit_code,
                       self.other_ready.ready_queue_mut())
    }

    pub(super) fn reap_exited_task(&mut self, task_id : TaskId) -> Option<ExitedTask> {
        self.wait
            .reap_exited_task(&mut self.registry, task_id)
    }

    pub(super) fn reap_one_exited_task(&mut self) -> Option<ExitedTask> {
        self.wait
            .reap_one_exited_task(&mut self.registry)
    }

    pub(super) fn reap_one_exited_child(&mut self, parent_id : TaskId) -> Option<ExitedTask> {
        let task_id = self.registry
                          .find_exited_child(parent_id)?;
        self.wait
            .reap_exited_task(&mut self.registry, task_id)
    }

    pub(super) fn has_child(&self, parent_id : TaskId) -> bool {
        self.registry
            .has_child(parent_id)
    }

    pub(super) fn wake_one_in_wait_queue(&mut self, wait_queue_id : WaitQueueId) -> Option<TaskId> {
        self.wait
            .wake_one_in_wait_queue(&mut self.registry,
                                    wait_queue_id,
                                    self.other_ready.ready_queue_mut())
    }

    pub(super) fn wake_all_in_wait_queue(&mut self, wait_queue_id : WaitQueueId) -> usize {
        self.wait
            .wake_all_in_wait_queue(&mut self.registry,
                                    wait_queue_id,
                                    self.other_ready.ready_queue_mut())
    }

    pub(super) fn requeue_wait_queue(&mut self,
                                     from_wait_queue_id : WaitQueueId,
                                     to_wait_queue_id : WaitQueueId,
                                     wake_count : usize,
                                     requeue_count : usize)
                                     -> usize {
        self.wait
            .requeue_wait_queue(&mut self.registry,
                                from_wait_queue_id,
                                to_wait_queue_id,
                                wake_count,
                                requeue_count,
                                self.other_ready.ready_queue_mut())
    }

    pub(super) fn current_task_id(&self) -> Option<TaskId> {
        self.registry
            .current_task_id()
    }

    pub(super) fn current_task_snapshot(&self) -> Option<TaskSnapshot> {
        self.registry
            .current_task_snapshot()
    }

    pub(super) fn task_snapshot(&self, task_id : TaskId) -> Option<TaskSnapshot> {
        self.registry
            .task_snapshot(task_id)
    }

    pub(super) fn fork_current(&mut self,
                               child_stack : usize,
                               new_aspace_ptr : usize,
                               new_satp : usize)
                               -> Option<TaskId> {
        let child_id = self.registry
                           .fork_current(child_stack, new_aspace_ptr, new_satp)?;
        self.other_ready
            .enqueue_ready_task(child_id);
        Some(child_id)
    }

    pub(super) fn clone_current_thread(&mut self,
                                       child_stack : usize,
                                       tls : usize,
                                       set_tls : bool)
                                       -> Option<TaskId> {
        let child_id = self.registry
                           .clone_current_thread(child_stack, tls, set_tls)?;
        self.other_ready
            .enqueue_ready_task(child_id);
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
                                 stack_info : task_api::UserStack) {
        self.registry
            .execve_current(entry_pc,
                            sp,
                            argc,
                            argv,
                            envp,
                            satp,
                            user_aspace_ptr,
                            image_info,
                            stack_info);
    }

    pub(super) fn current_tick(&self) -> TaskTick {
        self.wait
            .current_tick()
    }

    pub(super) fn current_task_kernel_stack_top(&self) -> Option<usize> {
        self.registry
            .current_task_kernel_stack_top()
    }

    pub(super) fn current_task_address_space_raw(&self) -> usize {
        self.registry
            .current_task_address_space_raw()
    }

    pub(super) fn current_task_user_aspace_ptr(&self) -> usize {
        self.registry
            .current_task_user_aspace_ptr()
    }

    pub(super) fn current_task_user_address_space_token(&self) -> usize {
        self.registry
            .current_task_user_address_space_token()
    }

    pub(super) fn current_task_trap_return_address_space_token(&self) -> usize {
        self.registry
            .current_task_trap_return_address_space_token()
    }

    pub(super) fn begin_current_trap_frame_access(&mut self,
                                                  trap_frame : TaskTrapFrame)
                                                  -> Option<*mut TaskTrapFrame> {
        self.registry
            .begin_current_trap_frame_access(trap_frame)
    }

    pub(super) fn restore_current_trap_frame(&self, trap_frame : &mut TaskTrapFrame) -> bool {
        self.registry
            .restore_current_trap_frame(trap_frame)
    }

    pub(super) fn take_current_wait_result(&mut self) -> TaskWaitResult {
        self.registry
            .take_current_wait_result()
    }
}

impl SwitchScheduler for RoundRobinScheduler {
    fn prepare_first_switch(&mut self) -> SwitchPair {
        RoundRobinScheduler::prepare_first_switch(self)
    }

    fn schedule(&mut self, reason : ScheduleReason) -> Option<SwitchPair> {
        RoundRobinScheduler::schedule(self, reason)
    }

    fn schedule_wait(&mut self,
                     wait_handle : TaskWaitHandle,
                     timeout_ticks : Option<TaskTick>)
                     -> Option<SwitchPair> {
        RoundRobinScheduler::schedule_wait(self, wait_handle, timeout_ticks)
    }
}
