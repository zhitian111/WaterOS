//! [`RoundRobinScheduler`]：把 `ScheduleReason` 与等待路径翻译成队列操作与
//! `__switch` 所需的上下文指针对。
//!
//! Idle 任务不占时间片：tick/yield 时若下一就绪任务仍是 idle，则不发起切换（见
//! `schedule` 分支）。
//!
//! **时间片控制**：`MAX_TICKS_PER_TASK` 定义了每个任务在被 Tick 抢占前可连续
//! 运行的 tick 数。增大此值可使调度行为更接近 FCFS。

use api_v0::ScheduleReason;
use task_api::{
    ExitedTask, KernelTaskEntry, TaskBlockReason, TaskId, TaskSnapshot, TaskTick, TaskWaitHandle,
    TaskWaitResult, UserTask, WaitQueueId,
};

use crate::queues::{QueueTarget, RoundRobinQueues};
use crate::registry::TaskRegistry;
use crate::{SwitchPair, TaskTrapFrame};

/// 每个任务在 Tick 抢占前可连续运行的逻辑 tick 数，定义在 `base-config::task`。
use config::task::MAX_TICKS_PER_TASK;

/// 就绪 FIFO + 阻塞/睡眠/等待超时组合下的具体调度器状态机。
pub(super) struct RoundRobinScheduler {
    registry : TaskRegistry,
    queues : RoundRobinQueues,
    /// 当前任务已连续运行的 tick 计数（未被 Block/Sleep/Yield 打断）。
    current_task_ticks : u64,
}

impl RoundRobinScheduler {
    pub(super) fn new() -> Self {
        Self { registry : TaskRegistry::new(),
               queues : RoundRobinQueues::new(),
               current_task_ticks : 0 }
    }

    pub(super) fn init(&mut self) {
        self.registry.init();
        self.queues.init();
        self.current_task_ticks = 0;
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

    pub(super) fn spawn_user_task_spec(&mut self, spec : UserTask) -> TaskId {
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
        self.current_task_ticks = 0;
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
                self.current_task_ticks = self.current_task_ticks
                                              .saturating_add(1);
                // 当前任务尚未达到时间片上限：tick 仅推进时钟，不触发抢占
                if self.current_task_ticks < MAX_TICKS_PER_TASK {
                    self.queues
                        .promote_sleeping_tasks(&mut self.registry);
                    self.queues
                        .promote_wait_timeouts(&mut self.registry);
                    return None;
                }
                // 时间片耗尽，重置计数器，下面执行抢占
                self.current_task_ticks = 0;
            }
            ScheduleReason::Sleep(ticks) if ticks == 0 => {
                return self.schedule(ScheduleReason::Yield);
            }
            _ => {
                // 非 Tick 原因（Yield/Block/Sleep/Exit）表明当前任务主动让出或阻塞，
                // 下一个被调度的任务重新从头计数。
                self.current_task_ticks = 0;
            }
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
        // 等待/阻塞后切换到的下一个任务从头计时间片
        self.current_task_ticks = 0;

        self.queues
            .promote_sleeping_tasks(&mut self.registry);
        self.queues
            .promote_wait_timeouts(&mut self.registry);

        if self.registry
               .wait_target_ready(wait_handle)
        {
            // 目标已就绪：不切换，仅标记当前任务等待结果为已唤醒（例如等待的任务已退出）。
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

    pub(super) fn reap_one_exited_child(&mut self, parent_id : TaskId) -> Option<ExitedTask> {
        let task_id = self.registry
                          .find_exited_child(parent_id)?;
        self.queues
            .reap_exited_task(&mut self.registry, task_id)
    }

    pub(super) fn has_child(&self, parent_id : TaskId) -> bool {
        self.registry
            .has_child(parent_id)
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
        self.queues
            .push_spawned_task(child_id);
        Some(child_id)
    }

    pub(super) fn current_tick(&self) -> TaskTick {
        self.queues
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
