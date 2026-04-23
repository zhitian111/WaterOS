use task_api::{
    ExitedTask, KernelTaskEntry, ScheduleReason, TaskBlockReason, TaskId, TaskSnapshot, TaskTick,
    TaskTrapFrame, TaskWaitHandle, TaskWaitResult, UserTaskSpec, WaitQueueId,
};

use crate::queues::{QueueTarget, RoundRobinQueues};
use crate::registry::TaskRegistry;
use crate::SwitchPair;

pub(super) struct RoundRobinScheduler {
    registry : TaskRegistry,
    queues : RoundRobinQueues,
}

impl RoundRobinScheduler {
    pub(super) fn new() -> Self {
        Self { registry : TaskRegistry::new(),
               queues : RoundRobinQueues::new() }
    }

    pub(super) fn init(&mut self) {
        self.registry.init();
        self.queues.init();
    }

    pub(super) fn spawn_kernel_task(&mut self, entry : KernelTaskEntry, arg : usize) -> TaskId {
        let task_id = self.registry
                          .spawn_kernel_task(entry, arg);
        self.queues
            .push_spawned_task(task_id);
        log::debug!("[task-scheduler] spawned task {}",
                    task_id);
        task_id
    }

    pub(super) fn spawn_user_task_spec(&mut self, spec : UserTaskSpec) -> TaskId {
        let task_id = self.registry
                          .spawn_user_task_spec(spec);
        self.queues
            .push_spawned_task(task_id);
        log::debug!("[task-scheduler] spawned user task {}",
                    task_id);
        task_id
    }

    pub(super) fn allocate_wait_queue(&mut self) -> WaitQueueId {
        self.queues
            .allocate_wait_queue()
    }

    pub(super) fn prepare_first_switch(&mut self) -> SwitchPair {
        self.queues
            .promote_sleeping_tasks(&mut self.registry);
        self.queues
            .promote_wait_timeouts(&mut self.registry);
        let next_task_id = self.queues
                               .pick_next_task_id();
        self.registry
            .first_switch_to(next_task_id)
    }

    pub(super) fn schedule(&mut self, reason : ScheduleReason) -> Option<SwitchPair> {
        match reason {
            ScheduleReason::Tick => {
                self.queues
                    .on_tick();
                self.registry
                    .account_tick_for_current();
            }
            ScheduleReason::Sleep(ticks) if ticks == 0 => {
                return self.schedule(ScheduleReason::Yield);
            }
            _ => {}
        }

        self.queues
            .promote_sleeping_tasks(&mut self.registry);
        self.queues
            .promote_wait_timeouts(&mut self.registry);

        let (current_task_id, current_ptr) = self.registry
                                                 .take_current_switch_out()?;

        if self.registry
               .is_idle(current_task_id)
        {
            let next_task_id = self.queues
                                   .pick_next_task_id();
            if next_task_id == current_task_id {
                let _ = self.registry
                            .mark_running_and_set_current(next_task_id);
                return None;
            }
            let next_ptr = self.registry
                               .mark_running_and_set_current(next_task_id);
            return Some((current_ptr, next_ptr));
        }

        let queue_target = match reason {
            ScheduleReason::StartFirst => QueueTarget::Ready,
            ScheduleReason::Yield | ScheduleReason::Tick => QueueTarget::Ready,
            ScheduleReason::Block(block_reason) => QueueTarget::Blocked(block_reason),
            ScheduleReason::Sleep(ticks) => {
                let wake_tick = self.queues
                                    .current_tick()
                                    .saturating_add(ticks.max(1));
                QueueTarget::Sleeping(wake_tick)
            }
            ScheduleReason::Exit(exit_code) => QueueTarget::Exited(exit_code),
        };

        self.queues
            .enqueue_task(&mut self.registry,
                          current_task_id,
                          queue_target);

        let next_task_id = self.queues
                               .pick_next_task_id();
        if next_task_id == current_task_id {
            let _ = self.registry
                        .mark_running_and_set_current(next_task_id);
            return None;
        }

        let next_ptr = self.registry
                           .mark_running_and_set_current(next_task_id);
        Some((current_ptr, next_ptr))
    }

    pub(super) fn schedule_wait(&mut self,
                                wait_handle : TaskWaitHandle,
                                timeout_ticks : Option<TaskTick>)
                                -> Option<SwitchPair> {
        self.queues
            .promote_sleeping_tasks(&mut self.registry);
        self.queues
            .promote_wait_timeouts(&mut self.registry);

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
        self.queues
            .enqueue_task(&mut self.registry,
                          current_task_id,
                          QueueTarget::Blocked(TaskBlockReason::Wait(wait_handle)));
        if let Some(timeout_ticks) = timeout_ticks {
            let wake_tick = self.queues
                                .current_tick()
                                .saturating_add(timeout_ticks.max(1));
            self.queues
                .enqueue_wait_timeout(current_task_id, wait_handle, wake_tick);
        }

        let next_task_id = self.queues
                               .pick_next_task_id();
        let next_ptr = self.registry
                           .mark_running_and_set_current(next_task_id);
        Some((current_ptr, next_ptr))
    }

    pub(super) fn wake_task(&mut self, task_id : TaskId) -> bool {
        self.queues
            .wake_task(&mut self.registry, task_id)
    }

    pub(super) fn reap_exited_task(&mut self, task_id : TaskId) -> Option<ExitedTask> {
        self.queues
            .reap_exited_task(&mut self.registry, task_id)
    }

    pub(super) fn reap_one_exited_task(&mut self) -> Option<ExitedTask> {
        self.queues
            .reap_one_exited_task(&mut self.registry)
    }

    pub(super) fn wake_one_in_wait_queue(&mut self, wait_queue_id : WaitQueueId) -> Option<TaskId> {
        self.queues
            .wake_one_in_wait_queue(&mut self.registry, wait_queue_id)
    }

    pub(super) fn wake_all_in_wait_queue(&mut self, wait_queue_id : WaitQueueId) -> usize {
        self.queues
            .wake_all_in_wait_queue(&mut self.registry, wait_queue_id)
    }

    pub(super) fn current_task_id(&self) -> Option<TaskId> {
        self.registry
            .current_task_id()
    }

    pub(super) fn current_task_snapshot(&self) -> Option<TaskSnapshot> {
        self.registry
            .current_task_snapshot()
    }

    pub(super) fn current_task_kernel_stack_top(&self) -> Option<usize> {
        self.registry
            .current_task_kernel_stack_top()
    }

    pub(super) fn record_current_trap_frame(&mut self, trap_frame : TaskTrapFrame) {
        self.registry
            .record_current_trap_frame(trap_frame);
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
