//! [`MultiClassScheduler`]：`SCHED_OTHER` + `SCHED_FIFO` + `SCHED_RR` 多类调度。

extern crate alloc;

use crate::queues::OtherReadyQueue;
use crate::rt_fifo_queue::RtFifoRunQueue;
use crate::rt_rr_queue::{RrTickAction, RtRrRunQueue};
use alloc::collections::VecDeque;
use api_v0::{
    QueueTarget, ReadyQueue, ReadyTaskSink, SchedPolicyChangeAction, SwitchScheduler, TaskRegistry,
    WaitQueues,
};
use arch::task::ActiveArchTaskContext as TaskContext;
use config::task::MAX_TICKS_PER_TASK;
use task_api::{
    ExitedTask, KernelTaskEntry, SchedError, SchedParam, SchedPolicy, TaskBlockReason,
    TaskExitCode, TaskId, TaskSnapshot, TaskState, TaskTick, TaskWaitHandle, TaskWaitResult,
    UserTask, WaitQueueId, IDLE_TASK_ID,
};

use crate::{SwitchPair, TaskTrapFrame};

use api_v0::ScheduleReason;

pub(super) struct MultiClassScheduler {
    registry : TaskRegistry,
    wait : WaitQueues,
    other_ready : OtherReadyQueue,
    fifo_ready : RtFifoRunQueue,
    rr_ready : RtRrRunQueue,
    current_task_ticks : u64,
}

impl MultiClassScheduler {
    pub(super) fn new() -> Self {
        Self { registry : TaskRegistry::new(),
               wait : WaitQueues::new(),
               other_ready : OtherReadyQueue::new(),
               fifo_ready : RtFifoRunQueue::new(),
               rr_ready : RtRrRunQueue::new(),
               current_task_ticks : 0 }
    }

    pub(super) fn init(&mut self) {
        self.registry.init();
        self.wait.init();
        self.other_ready
            .init();
        self.fifo_ready = RtFifoRunQueue::new();
        self.rr_ready = RtRrRunQueue::new();
        self.current_task_ticks = 0;
    }

    fn detach_from_run_queues(&mut self, task_id : TaskId) {
        self.other_ready
            .detach_task(task_id);
        self.fifo_ready
            .remove(task_id);
        self.rr_ready
            .remove(task_id);
    }

    fn enqueue_ready_by_policy(&mut self, task_id : TaskId) {
        let Some(snap) = self.registry
                             .task_snapshot(task_id)
        else {
            return;
        };
        match snap.sched_policy {
            SchedPolicy::Other => self.other_ready
                                      .enqueue_ready_task(task_id),
            SchedPolicy::Fifo => self.fifo_ready
                                     .enqueue(task_id, snap.sched_priority),
            SchedPolicy::Rr => self.rr_ready
                                   .on_task_unblocked(task_id, snap.sched_priority),
        }
    }

    fn drain_staging(&mut self, staging : &mut VecDeque<TaskId>) {
        while let Some(task_id) = staging.pop_front() {
            self.enqueue_ready_by_policy(task_id);
        }
    }

    fn promote_sleep_and_timeouts(&mut self) {
        let mut staging = VecDeque::new();
        self.wait
            .promote_sleeping_tasks(&mut self.registry, &mut staging);
        self.wait
            .promote_wait_timeouts(&mut self.registry, &mut staging);
        self.drain_staging(&mut staging);
    }

    fn highest_ready_rt_priority(&self) -> Option<i32> {
        match (self.fifo_ready
                   .highest_runnable_priority(&self.registry),
               self.rr_ready
                   .highest_ready_priority(&self.registry))
        {
            (Some(fifo), Some(rr)) => Some(fifo.max(rr)),
            (fifo, rr) => fifo.or(rr),
        }
    }

    fn ready_task_should_preempt(&self, current_id : TaskId, current : TaskSnapshot) -> bool {
        if self.registry
               .is_idle(current_id)
        {
            return self.highest_ready_rt_priority()
                       .is_some() ||
                   self.other_ready
                       .has_runnable(&self.registry);
        }

        match current.sched_policy {
            SchedPolicy::Other => self.highest_ready_rt_priority()
                                      .is_some(),
            SchedPolicy::Fifo | SchedPolicy::Rr => {
                self.highest_ready_rt_priority()
                    .is_some_and(|priority| priority > current.sched_priority)
            }
        }
    }

    fn clear_rr_if_yielding(&mut self, current_id : TaskId) {
        if let Some(snap) = self.registry
                                .task_snapshot(current_id)
        {
            if snap.sched_policy == SchedPolicy::Rr {
                self.rr_ready
                    .clear_running();
            }
        }
    }

    fn pick_next_runnable(&mut self) -> TaskId {
        if let Some(current_id) = self.registry
                                      .current_task_id()
        {
            if let Some(snap) = self.registry
                                    .task_snapshot(current_id)
            {
                if snap.sched_policy == SchedPolicy::Rr {
                    if self.rr_ready
                           .should_continue_current(current_id, snap.sched_priority)
                    {
                        return current_id;
                    }
                }
            }
        }

        for priority in (1..=99).rev() {
            if let Some(task_id) = self.fifo_ready
                                       .pop_front_at_priority(priority, &self.registry)
            {
                self.rr_ready
                    .clear_running();
                return task_id;
            }
            if let Some(task_id) = self.rr_ready
                                       .pick_at_priority(priority, &self.registry)
            {
                return task_id;
            }
        }

        self.rr_ready
            .clear_running();
        let other = self.other_ready
                        .pick_next_runnable_task_id(&self.registry);
        other.unwrap_or(IDLE_TASK_ID)
    }

    fn sched_class(policy : SchedPolicy) -> u8 {
        match policy {
            SchedPolicy::Other => 0,
            SchedPolicy::Fifo | SchedPolicy::Rr => 1,
        }
    }

    fn beats_running(challenger_policy : SchedPolicy,
                     challenger_priority : i32,
                     runner_policy : SchedPolicy,
                     runner_priority : i32)
                     -> bool {
        let challenger_class = Self::sched_class(challenger_policy);
        let runner_class = Self::sched_class(runner_policy);
        if challenger_class > runner_class {
            return true;
        }
        if challenger_class < runner_class {
            return false;
        }
        challenger_priority > runner_priority
    }

    pub(super) fn apply_sched_policy_change(&mut self,
                                            task_id : TaskId,
                                            policy : SchedPolicy,
                                            param : SchedParam)
                                            -> Result<SchedPolicyChangeAction, SchedError> {
        let old_snap = self.registry
                           .task_snapshot(task_id)
                           .ok_or(SchedError::NoSuchTask)?;
        let was_ready = old_snap.state == TaskState::Ready;

        self.detach_from_run_queues(task_id);
        if !self.registry
                .set_task_sched(task_id, policy, param.priority)
        {
            return Err(SchedError::NoSuchTask);
        }

        if was_ready {
            self.enqueue_ready_by_policy(task_id);
        }

        if let Some(current_id) = self.registry
                                      .current_task_id()
        {
            if current_id != task_id {
                let new_snap = self.registry
                                   .task_snapshot(task_id)
                                   .expect("task exists after set_task_sched");
                let cur_snap = self.registry
                                   .task_snapshot(current_id)
                                   .expect("current task exists");
                if Self::beats_running(new_snap.sched_policy,
                                       new_snap.sched_priority,
                                       cur_snap.sched_policy,
                                       cur_snap.sched_priority)
                {
                    return Ok(SchedPolicyChangeAction::RescheduleNow);
                }
            }
        }
        Ok(SchedPolicyChangeAction::NoReschedule)
    }

    pub(super) fn spawn_kernel_task(&mut self, entry : KernelTaskEntry, arg : usize) -> TaskId {
        let task_id = self.registry
                          .spawn_kernel_task(entry, arg);
        self.enqueue_ready_by_policy(task_id);
        log::debug!("[task-scheduler] spawned task {}",
                    task_id);
        task_id
    }

    pub(super) fn spawn_user_task_spec(&mut self, spec : UserTask) -> TaskId {
        let task_id = self.registry
                          .spawn_user_task_spec(spec);
        self.enqueue_ready_by_policy(task_id);
        log::debug!("[task-scheduler] spawned user task {}",
                    task_id);
        task_id
    }

    pub(super) fn allocate_wait_queue(&mut self) -> WaitQueueId {
        self.wait
            .allocate_wait_queue()
    }

    pub(super) fn prepare_first_switch(&mut self) -> SwitchPair {
        self.promote_sleep_and_timeouts();
        let next_task_id = self.pick_next_runnable();
        self.current_task_ticks = 0;
        self.registry
            .first_switch_to(next_task_id)
    }

    fn finish_schedule_switch(&mut self,
                              current_task_id : TaskId,
                              current_ptr : *mut TaskContext,
                              is_exit : bool)
                              -> Option<SwitchPair> {
        let next_task_id = self.pick_next_runnable();
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
            if let Some(snap) = self.registry
                                    .task_snapshot(next_task_id)
            {
                if snap.sched_policy == SchedPolicy::Rr {
                    self.rr_ready
                        .note_running(next_task_id, snap.sched_priority);
                }
            }
            let _ = self.registry
                        .mark_running_and_set_current(next_task_id);
            return None;
        }
        if let Some(snap) = self.registry
                                .task_snapshot(next_task_id)
        {
            if snap.sched_policy == SchedPolicy::Rr {
                self.rr_ready
                    .note_running(next_task_id, snap.sched_priority);
            }
        }
        let next_ptr = self.registry
                           .mark_running_and_set_current(next_task_id);
        Some((current_ptr, next_ptr))
    }

    pub(super) fn schedule(&mut self, reason : ScheduleReason) -> Option<SwitchPair> {
        match reason {
            ScheduleReason::Tick => {
                self.wait.on_tick();
                self.registry
                    .account_tick_for_current();

                let current = self.registry
                                  .current_task_id()
                                  .and_then(|task_id| {
                                      self.registry
                                          .task_snapshot(task_id)
                                          .map(|snapshot| (task_id, snapshot))
                                  });
                let quantum_expired =
                    current.map(|(current_id, snap)| match snap.sched_policy {
                                    SchedPolicy::Other => {
                                        self.current_task_ticks = self.current_task_ticks
                                                                      .saturating_add(1);
                                        self.current_task_ticks >= MAX_TICKS_PER_TASK
                                    }
                                    SchedPolicy::Rr => {
                                        matches!(self.rr_ready
                                                     .on_tick_current(current_id,
                                                                      snap.sched_priority),
                                                 RrTickAction::YieldToSamePriority)
                                    }
                                    SchedPolicy::Fifo => false,
                                })
                           .unwrap_or(false);

                self.promote_sleep_and_timeouts();

                let ready_preempts = current.is_some_and(|(current_id, snap)| {
                                                self.ready_task_should_preempt(current_id, snap)
                                            });
                if !quantum_expired && !ready_preempts {
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

        if !matches!(reason, ScheduleReason::Tick) {
            self.promote_sleep_and_timeouts();
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
            let next_task_id = self.pick_next_runnable();
            if next_task_id == current_task_id {
                let _ = self.registry
                            .mark_running_and_set_current(next_task_id);
                return None;
            }
            if let Some(snap) = self.registry
                                    .task_snapshot(next_task_id)
            {
                if snap.sched_policy == SchedPolicy::Rr {
                    self.rr_ready
                        .note_running(next_task_id, snap.sched_priority);
                }
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
            self.detach_from_run_queues(current_task_id);
        }

        if matches!(queue_target, QueueTarget::Ready) {
            self.clear_rr_if_yielding(current_task_id);
        }

        let mut staging = VecDeque::new();
        self.wait
            .enqueue_task(&mut self.registry,
                          current_task_id,
                          queue_target,
                          &mut staging);
        self.drain_staging(&mut staging);

        self.finish_schedule_switch(current_task_id, current_ptr, is_exit)
    }

    pub(super) fn schedule_wait(&mut self,
                                wait_handle : TaskWaitHandle,
                                timeout_ticks : Option<TaskTick>)
                                -> Option<SwitchPair> {
        self.current_task_ticks = 0;
        self.promote_sleep_and_timeouts();

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
        self.detach_from_run_queues(current_task_id);

        let mut staging = VecDeque::new();
        self.wait
            .enqueue_task(&mut self.registry,
                          current_task_id,
                          QueueTarget::Blocked(TaskBlockReason::Wait(wait_handle)),
                          &mut staging);
        self.drain_staging(&mut staging);

        if let Some(timeout_ticks) = timeout_ticks {
            let wake_tick = self.wait
                                .current_tick()
                                .saturating_add(timeout_ticks.max(1));
            self.wait
                .enqueue_wait_timeout(current_task_id, wait_handle, wake_tick);
        }

        let next_task_id = self.pick_next_runnable();
        if let Some(snap) = self.registry
                                .task_snapshot(next_task_id)
        {
            if snap.sched_policy == SchedPolicy::Rr {
                self.rr_ready
                    .note_running(next_task_id, snap.sched_priority);
            }
        }
        let next_ptr = self.registry
                           .mark_running_and_set_current(next_task_id);
        Some((current_ptr, next_ptr))
    }

    pub(super) fn wake_task(&mut self, task_id : TaskId) -> bool {
        let mut staging = VecDeque::new();
        let woken = self.wait
                        .wake_task(&mut self.registry,
                                   task_id,
                                   &mut staging);
        self.drain_staging(&mut staging);
        woken
    }

    pub(super) fn interrupt_task(&mut self, task_id : TaskId) -> bool {
        let mut staging = VecDeque::new();
        let interrupted = self.wait
                              .interrupt_task(&mut self.registry,
                                              task_id,
                                              &mut staging);
        self.drain_staging(&mut staging);
        interrupted
    }

    pub(super) fn kill_task(&mut self, task_id : TaskId, exit_code : TaskExitCode) -> bool {
        self.detach_from_run_queues(task_id);
        let mut staging = VecDeque::new();
        let killed = self.wait
                         .kill_task(&mut self.registry,
                                    task_id,
                                    exit_code,
                                    &mut staging);
        self.drain_staging(&mut staging);
        killed
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
        let mut staging = VecDeque::new();
        let task_id = self.wait
                          .wake_one_in_wait_queue(&mut self.registry,
                                                  wait_queue_id,
                                                  &mut staging);
        self.drain_staging(&mut staging);
        task_id
    }

    pub(super) fn wake_all_in_wait_queue(&mut self, wait_queue_id : WaitQueueId) -> usize {
        let mut staging = VecDeque::new();
        let count = self.wait
                        .wake_all_in_wait_queue(&mut self.registry,
                                                wait_queue_id,
                                                &mut staging);
        self.drain_staging(&mut staging);
        count
    }

    pub(super) fn requeue_wait_queue(&mut self,
                                     from_wait_queue_id : WaitQueueId,
                                     to_wait_queue_id : WaitQueueId,
                                     wake_count : usize,
                                     requeue_count : usize)
                                     -> usize {
        let mut staging = VecDeque::new();
        let count = self.wait
                        .requeue_wait_queue(&mut self.registry,
                                            from_wait_queue_id,
                                            to_wait_queue_id,
                                            wake_count,
                                            requeue_count,
                                            &mut staging);
        self.drain_staging(&mut staging);
        count
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
        self.enqueue_ready_by_policy(child_id);
        Some(child_id)
    }

    pub(super) fn clone_current_thread(&mut self,
                                       child_stack : usize,
                                       tls : usize,
                                       set_tls : bool)
                                       -> Option<TaskId> {
        let child_id = self.registry
                           .clone_current_thread(child_stack, tls, set_tls)?;
        self.enqueue_ready_by_policy(child_id);
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

impl SwitchScheduler for MultiClassScheduler {
    fn prepare_first_switch(&mut self) -> SwitchPair {
        MultiClassScheduler::prepare_first_switch(self)
    }

    fn schedule(&mut self, reason : ScheduleReason) -> Option<SwitchPair> {
        MultiClassScheduler::schedule(self, reason)
    }

    fn schedule_wait(&mut self,
                     wait_handle : TaskWaitHandle,
                     timeout_ticks : Option<TaskTick>)
                     -> Option<SwitchPair> {
        MultiClassScheduler::schedule_wait(self, wait_handle, timeout_ticks)
    }
}
