//! [`MultiClassScheduler`]：`SCHED_OTHER` + `SCHED_FIFO` + `SCHED_RR` 多类调度。

extern crate alloc;
mod cpu;
mod lifecycle;
mod policy;
mod tasks;
mod wait;
use crate::{SwitchPair, TaskTrapFrame};
use api_v0::{CPUState, CpuSnapshot, QueueTarget, TaskRegistry, WaitQueues};
use arch::task::{ActiveArchTaskContext as TaskContext, ArchTaskContext};
use base::cpu::CpuMask;
use config::task::MAX_CPUS;
use task_api::{
    AddressSpaceHandle, CpuId, ExitedTask, KernelTaskEntry, Priority, SchedError, SchedPolicy,
    TaskExitCode, TaskId, TaskSnapshot, TaskState, TaskTick, TaskWaitResult, TaskWaitTarget,
    UserTask, WaitQueueId,
};

use api_v0::ScheduleReason;

unsafe extern "C" {
    static kernel_heap_start : u8;
    static kernel_heap_end : u8;
}
pub(super) struct MultiClassScheduler {
    pub registry : TaskRegistry,
    pub wait_queues : WaitQueues,
    pub cpu_states : [CPUState; MAX_CPUS],
    /// 环形选核的起点。负载相同时，从这里开始的第一个 online CPU 获胜。
    pub next_placement_cpu : usize,
    /// 唯一推进全局 sleep/wait timeout 时间的 BSP。
    pub timekeeper_cpu : Option<CpuId>,
    /// 入队时在 scheduler 锁内累计，锁外再实际发送定向 IPI。
    pub pending_reschedule_cpus : CpuMask,
}


impl MultiClassScheduler {
    // ================================================================
    //  构造与初始化
    // ================================================================
    pub(super) fn new() -> Self {
        Self { registry : TaskRegistry::new(),
               wait_queues : WaitQueues::new(),
               cpu_states : core::array::from_fn(|i| CPUState::new(CpuId::from_raw(i))),
               next_placement_cpu : 0,
               timekeeper_cpu : None,
               pending_reschedule_cpus : CpuMask::EMPTY }
    }

    pub(super) fn init(&mut self, boot_cpu : CpuId) {
        self.registry.init();
        self.wait_queues
            .init();
        self.next_placement_cpu = 0;
        self.timekeeper_cpu = None;
        self.pending_reschedule_cpus = CpuMask::EMPTY;
        // 为每个 CPU 创建 idle 任务
        for (cpu_id, cpu_state) in self.cpu_states
                                       .iter_mut()
                                       .enumerate()
        {
            let cpu_id = CpuId::from_raw(cpu_id);
            cpu_state.init(cpu_id);
            cpu_state.set_online(boot_cpu == cpu_id);
            let idle_id = self.registry
                              .spawn_idle_task();
            cpu_state.set_idle_task_id(idle_id);
            // The idle task is the initial logical current task on every CPU.
            // Seed the complete cache here: the first switch saves the old
            // context through `current_task_cx`, so setting its ID alone
            // would make that save target a null pointer.
            let idle_snapshot = self.registry
                                    .task_snapshot(idle_id);
            cpu_state.set_current_task(&idle_snapshot);
        }
    }

    // ================================================================
    //  核心调度
    // ================================================================

    /// 标记任务为 Running 并更新当前 CPU 的 current_task_id。
    fn set_current_task(&mut self, snap : &TaskSnapshot, cpu_id : CpuId) {
        if let Some(running_cpu) = snap.running_cpu_id {
            assert_eq!(running_cpu,
                       cpu_id,
                       "[scheduler] task {} is already running on CPU {} while CPU {} selected it",
                       snap.id,
                       running_cpu.raw(),
                       cpu_id.raw());
        }
        let cpu_state = &mut self.cpu_states[cpu_id.raw()];
        let previous_aspace = cpu_state.current_aspace;
        if previous_aspace != snap.user_aspace_ptr {
            mm_api::user_aspace_lifecycle::notify_aspace_cpu_leave(previous_aspace, cpu_id);
            mm_api::user_aspace_lifecycle::notify_aspace_cpu_enter(snap.user_aspace_ptr, cpu_id);
        }
        cpu_state.set_current_task(snap);
        self.registry
            .mark_running(snap.id, cpu_id);
    }

    /// 在实际 `__switch` 前验证恢复目标上下文。返回地址落在 kernel heap
    /// 必然是上下文损坏：heap 数据绝不应被当作指令执行。
    fn validate_switch_target(&self, task_id : TaskId, cx : *const TaskContext, cpu_id : CpuId) {
        assert!(!cx.is_null(),
                "[scheduler] null next context: cpu={} task={}",
                cpu_id.raw(),
                task_id);
        let context = unsafe { &*cx };
        let ra = context.return_address();
        let sp = context.stack_pointer();
        let heap_start = core::ptr::addr_of!(kernel_heap_start) as usize;
        let heap_end = core::ptr::addr_of!(kernel_heap_end) as usize;
        if (heap_start..heap_end).contains(&ra) {
            panic!("[scheduler] corrupted switch target: cpu={} task={} cx={:#x} ra={:#x} sp={:#x} heap=[{:#x},{:#x})",
                   cpu_id.raw(),
                   task_id,
                   cx as usize,
                   ra,
                   sp,
                   heap_start,
                   heap_end);
        }
    }

    /// 构造切换对，并在唯一的出口处校验恢复目标。
    fn switch_pair(&self,
                   current : *mut TaskContext,
                   next_task_id : TaskId,
                   next : *const TaskContext,
                   cpu_id : CpuId)
                   -> SwitchPair {
        self.validate_switch_target(next_task_id, next, cpu_id);
        (current, next)
    }

    /// 首次任务切换（冷启动入口）。
    pub(super) fn prepare_first_switch(&mut self, cpu_id : CpuId) -> SwitchPair {
        let next_task_id = self.cpu_states[cpu_id.raw()].pick_next_runnable();
        let snap = self.registry
                       .task_snapshot(next_task_id);
        self.set_current_task(&snap, cpu_id);
        let boot_task_cx = self.cpu_states[cpu_id.raw() as usize].boot_task_cx();
        self.switch_pair(boot_task_cx,
                         next_task_id,
                         snap.task_cx as *const TaskContext,
                         cpu_id)
    }

    /// 普通调度入口：根据 `reason` 决定是否切换当前任务。
    pub(super) fn schedule(&mut self,
                           reason : ScheduleReason,
                           cpu_id : CpuId)
                           -> Option<SwitchPair> {
        // Phase 1: 根据 reason 做前置处理
        match reason {
            ScheduleReason::Reschedule => self.cpu_states[cpu_id.raw()].cpu_should_reschedule()?,
            ScheduleReason::Tick => self.tick(cpu_id)?,
            // Yield / Block / Sleep / Exit：在选下一个任务之前确保所有到期任务已入队
            _ => {
                if self.is_timekeeper_cpu(cpu_id) {
                    self.enqueue_woken_and_timeout_tasks();
                }
            }
        }

        // Phase 3: 从 cpu_states 取出当前任务
        let current_task_id = self.cpu_states[cpu_id.raw()].current_task_id()
                                                           .expect("current task must exist");
        let current_ptr = self.cpu_states[cpu_id.raw()].current_task_cx;
        let next_task_id = self.cpu_states[cpu_id.raw()].pick_next_runnable();
        let snap = self.registry
                       .task_snapshot(next_task_id);

        // Phase 4: IDLE 特殊处理（不经过 enqueue）
        if self.cpu_states[cpu_id.raw()].is_current_idle() {
            if next_task_id == current_task_id {
                self.set_current_task(&snap, cpu_id);
                return None;
            }
            self.set_current_task(&snap, cpu_id);
            let next_ptr = snap.task_cx as *const TaskContext;
            return Some(self.switch_pair(current_ptr, next_task_id, next_ptr, cpu_id));
        }
        // Phase 5-8: 非 IDLE 调度
        let queue_target = self.pick_queue(reason);
        self.enqueue_task(queue_target, current_task_id, cpu_id);
        // Phase 8: 选下一个任务，决定是否 __switch
        let is_exit = matches!(reason, ScheduleReason::Exit(_));
        if next_task_id == current_task_id {
            if is_exit {
                let idle_id = self.cpu_states[cpu_id.raw()].idle_task_id?;
                let snap = self.registry
                               .task_snapshot(idle_id);
                self.set_current_task(&snap, cpu_id);
                let next_ptr = snap.task_cx as *const TaskContext;
                return Some(self.switch_pair(current_ptr, idle_id, next_ptr, cpu_id));
            }
            self.set_current_task(&snap, cpu_id);
            return None;
        }
        self.set_current_task(&snap, cpu_id);
        let next_ptr = snap.task_cx as *const TaskContext;
        Some(self.switch_pair(current_ptr, next_task_id, next_ptr, cpu_id))
    }


    /// Tick 前置处理：推进时间、检查时间片与抢占条件。
    fn tick(&mut self, cpu_id : CpuId) -> Option<()> {
        // 1. 推进全局 tick
        self.cpu_states[cpu_id.raw()].inc_timer_tick();
        if self.is_timekeeper_cpu(cpu_id) {
            self.wait_queues
                .tick();
        }
        // 2. 仅推进 CPU 本地缓存；任务统计会在离开 CPU 时统一回写 TCB。
        // 3. 推进当前任务的时间片/vruntime（由 CPUState::tick 按策略分发）
        self.cpu_states[cpu_id.raw()].tick();

        // 4. 检查时间片是否耗尽
        let Some(current_id) = self.cpu_states[cpu_id.raw()].current_task_id else {
            return None;
        };
        let quantum_expired = !self.cpu_states[cpu_id.raw()].is_current_runnable();

        // 5. 检查抢占
        let ready_preempts = self.cpu_states[cpu_id.raw()].ready_task_should_preempt();

        // 6. 处理唤醒/超时任务
        let needs_switch = quantum_expired || ready_preempts;
        if needs_switch ||
           self.wait_queues
               .has_woken_or_timeout_tasks()
        {
            if self.is_timekeeper_cpu(cpu_id) {
                self.enqueue_woken_and_timeout_tasks();
            }
        }
        if needs_switch {
            Some(())
        } else {
            None
        }
    }
    pub fn pick_queue(&mut self, reason : ScheduleReason) -> QueueTarget {
        match reason {
            ScheduleReason::StartFirst |
            ScheduleReason::Yield |
            ScheduleReason::Tick |
            ScheduleReason::Reschedule => QueueTarget::Ready,
            ScheduleReason::Block(block_reason) => QueueTarget::Blocked(block_reason),
            ScheduleReason::Sleep(ticks) => {
                if ticks == 0 {
                    QueueTarget::Ready
                } else {
                    let wake_tick = self.wait_queues
                                        .current_tick()
                                        .saturating_add(ticks);
                    QueueTarget::Sleeping(wake_tick)
                }
            }
            ScheduleReason::Exit(exit_code) => QueueTarget::Exited(exit_code),
        }
    }
    /// Phase 5-8: 非 IDLE 任务的完整调度路径（确定去向 → 摘除 → 入队 → 选下一个）。
    /// 等待调度入口：当前任务因等待某个 `target` 而阻塞。
    ///
    /// 如果目标已经就绪（`wait_target_ready` 返回 true），则无需阻塞，直接返回 `None`。
    /// 否则将当前任务放入等待队列 + 可选的超时队列，然后切换到下一个就绪任务。
    pub(super) fn schedule_wait(&mut self,
                                target : TaskWaitTarget,
                                timeout_ticks : Option<TaskTick>,
                                cpu_id : CpuId)
                                -> Option<SwitchPair> {
        // ===== Phase 1: 前置处理 =====
        if self.is_timekeeper_cpu(cpu_id) {
            self.enqueue_woken_and_timeout_tasks();
        }

        // ===== Phase 2: 快速路径 — 目标已就绪，无需阻塞 =====
        if self.registry
               .wait_target_ready(target)
        {
            if let Some(current_task_id) = self.cpu_states[cpu_id.raw()].current_task_id {
                self.registry
                    .finish_wait(current_task_id, TaskWaitResult::Woken);
            }
            return None;
        }

        // ===== Phase 3: 从 cpu_states 取出当前任务 =====
        let current_task_id = self.cpu_states[cpu_id.raw()].current_task_id?;
        let current_ptr = self.cpu_states[cpu_id.raw()].current_task_cx;
        self.cpu_states[cpu_id.raw()].dequeue(current_task_id);

        // ===== Phase 4: 将当前任务入队到等待队列 =====
        self.enqueue_task(QueueTarget::Blocked(target),
                          current_task_id,
                          cpu_id);

        // ===== Phase 5: 可选超时 =====
        if let Some(timeout_ticks) = timeout_ticks {
            let wake_tick = self.wait_queues
                                .current_tick()
                                .saturating_add(timeout_ticks.max(1));
            self.wait_queues
                .enqueue_wait_timeout(current_task_id, target, wake_tick);
        }

        // ===== Phase 6: 选下一个任务，直接切换（当前已阻塞） =====
        let next_task_id = self.cpu_states[cpu_id.raw()].pick_next_runnable();
        let snap = self.registry
                       .task_snapshot(next_task_id);
        self.set_current_task(&snap, cpu_id);
        let next_ptr = snap.task_cx as *const TaskContext;
        Some(self.switch_pair(current_ptr, next_task_id, next_ptr, cpu_id))
    }

    fn sync_current_to_registry(&mut self, cpu_id : CpuId) {
        let (current_task_id, policy, vruntime, runtime_ticks) = {
            let cpu = &mut self.cpu_states[cpu_id.raw()];
            let Some(current_task_id) = cpu.current_task_id() else {
                return;
            };
            let values = (current_task_id,
                          cpu.current_policy,
                          cpu.current_vruntime,
                          cpu.current_runtime_ticks);
            cpu.current_runtime_ticks = 0;
            values
        };
        if policy == SchedPolicy::Other {
            self.registry
                .set_vruntime(current_task_id, vruntime);
        }
        self.registry
            .add_ticks(current_task_id, runtime_ticks);
    }
    /// 在 scheduler 锁内将当前任务转换到目标状态。
    fn enqueue_task(&mut self, target : QueueTarget, current_task_id : TaskId, cpu_id : CpuId) {
        self.sync_current_to_registry(cpu_id);
        match target {
            QueueTarget::Ready => {
                // 通常 Yield/Tick 会回到当前 CPU；但若 affinity 在运行期间被
                // 改为排除当前 CPU，必须由本 CPU 的 Reschedule 路径把它放到
                // 允许的远端 runqueue，不能继续在禁止 CPU 上运行。
                let affinity = self.registry
                                   .task_snapshot(current_task_id)
                                   .affinity;
                let target_cpu = if affinity.contains(cpu_id) {
                    cpu_id
                } else {
                    self.pick_cpu_for_new_task(current_task_id)
                };
                self.enqueue_ready_by_cpu(current_task_id, target_cpu);
                if target_cpu != cpu_id {
                    self.request_reschedule(target_cpu);
                }
            }
            QueueTarget::Blocked(reason) => {
                self.registry
                    .clear_wait_result(current_task_id);
                self.registry
                    .mark_blocking(current_task_id, reason);
                self.wait_queues
                    .enqueue_wait_task(current_task_id, reason);
            }
            QueueTarget::Sleeping(wake_tick) => {
                self.registry
                    .clear_wait_result(current_task_id);
                self.registry
                    .mark_sleeping(current_task_id, wake_tick);
                self.wait_queues
                    .enqueue_sleep_task(current_task_id, wake_tick);
            }
            QueueTarget::Exited(exit_code) => {
                let waiters = self.wait_queues
                                  .wake_all_waiters_for_task_exit(current_task_id);
                // 唤醒所有等待当前任务退出的 waiters
                for waiter_id in &waiters {
                    self.registry
                        .finish_wait(*waiter_id, TaskWaitResult::Woken);
                    self.enqueue_woken_task(*waiter_id);
                }
                // 唤醒等待当前任务的父任务
                if let Some(parent_id) = self.registry
                                             .task_snapshot(current_task_id)
                                             .parent_id
                {
                    let child_waiters = self.wait_queues
                                            .wake_child_exit_waiters(parent_id);
                    for waiter_id in &child_waiters {
                        self.registry
                            .finish_wait(*waiter_id, TaskWaitResult::Woken);
                        self.enqueue_woken_task(*waiter_id);
                    }
                }
                self.wait_queues
                    .enqueue_exited_task(current_task_id);
                self.registry
                    .mark_exited(current_task_id, exit_code);
            }
        }
    }
    /// 选出一个 CPU 来放置新创建的任务（fork/clone/spawn）。
    pub(super) fn enqueue_ready_task(&mut self, task_id : TaskId) {
        let picked_cpu = self.pick_cpu_for_new_task(task_id);
        self.enqueue_ready_by_cpu(task_id, picked_cpu);
        self.request_reschedule(picked_cpu);
    }


    /// 将已阻塞任务优先放回其上次运行的 online CPU。
    ///
    /// `last_cpu_id` 不可用时才回退到新任务的最小负载选核策略。
    pub(super) fn enqueue_woken_task(&mut self, task_id : TaskId) -> CpuId {
        let snap = self.registry
                       .task_snapshot(task_id);
        let affinity = snap.affinity;
        let target = snap.last_cpu_id
                         .filter(|cpu_id| {
                             cpu_id.fits_capacity(self.cpu_states
                                                      .len())
                         })
                         .filter(|cpu_id| self.cpu_states[cpu_id.raw()].online)
                         .filter(|cpu_id| affinity.contains(*cpu_id))
                         .unwrap_or_else(|| self.pick_cpu_for_new_task(task_id));
        self.enqueue_ready_by_cpu(task_id, target);
        self.request_reschedule(target);
        target
    }

    fn request_reschedule(&mut self, cpu_id : CpuId) {
        self.cpu_states[cpu_id.raw()].need_resched = true;
        self.pending_reschedule_cpus
            .insert(cpu_id);
    }

    pub(super) fn take_pending_reschedule_cpus(&mut self) -> CpuMask {
        let pending = self.pending_reschedule_cpus;
        self.pending_reschedule_cpus = CpuMask::EMPTY;
        pending
    }

    /// 消费当前 CPU 的重调度请求；SSIP 没有请求位时不应触发调度。
    pub(super) fn take_need_resched(&mut self, cpu_id : CpuId) -> bool {
        let need_resched = self.cpu_states[cpu_id.raw()].need_resched;
        self.cpu_states[cpu_id.raw()].need_resched = false;
        need_resched
    }
    fn enqueue_ready_by_cpu(&mut self, task_id : TaskId, cpu_id : CpuId) {
        assert!(Some(task_id) != self.cpu_states[cpu_id.raw()].idle_task_id,
                "idle task must not be placed on a ready queue");
        assert!(self.cpu_states[cpu_id.raw()].online,
                "ready task must target an online CPU");
        assert!(self.registry
                    .get_affinity(task_id)
                    .expect("queued task must exist")
                    .contains(cpu_id),
                "ready task must target a CPU allowed by its affinity");
        if let Some(old_cpu_id) = self.registry
                                      .ready_cpu_id(task_id)
        {
            // 策略切换或防御性重复入队时，先根据 TCB 所记录的旧归属摘除。
            // 这样同一任务不会同时存在于两个 CPU 的 runqueue。
            self.cpu_states[old_cpu_id.raw()].dequeue(task_id);
        }
        let snap = self.ready_snapshot_for_cpu(task_id, cpu_id);
        self.registry
            .mark_ready(task_id, cpu_id);
        self.cpu_states[cpu_id.raw()].enqueue(task_id, &snap);
    }

    /// 准备进入 `cpu_id` 就绪队列的任务快照。
    ///
    /// CFS 的 vruntime 基线属于目标 CPU，不能在源 CPU 的
    /// `sync_current_to_registry` 中处理；该函数同时覆盖新建、唤醒和迁移任务。
    fn ready_snapshot_for_cpu(&mut self, task_id : TaskId, cpu_id : CpuId) -> TaskSnapshot {
        let mut snap = self.registry
                           .task_snapshot(task_id);
        if snap.policy != SchedPolicy::Other {
            return snap;
        }

        let normalized = self.cpu_states[cpu_id.raw()].cfs_queue
                                                       .normalize_vruntime(snap.vruntime);
        if normalized != snap.vruntime {
            self.registry
                .set_vruntime(task_id, normalized);
            snap.vruntime = normalized;
        }
        snap
    }

    /// 从所有 CPU 的就绪队列摘除任务（用于 kill / discard 等跨 CPU 操作）。
    fn dequeue_from_all_cpus(&mut self, task_id : TaskId) {
        for cpu in &mut self.cpu_states {
            cpu.dequeue(task_id);
        }
    }
    /// 到期睡眠/超时任务到就绪队列。(超时唤醒)
    fn enqueue_woken_and_timeout_tasks(&mut self) {
        for task_id in &self.wait_queues
                            .woken_tasks()
        {
            self.enqueue_woken_task(*task_id);
        }
        for (task_id, target) in &self.wait_queues
                                      .timeout_tasks()
        {
            let still_waiting = matches!(
                self.registry.state(*task_id),
                Some(TaskState::Blocking(t)) if t == *target
            );
            if !still_waiting {
                continue;
            }
            self.registry
                .finish_wait(*task_id, TaskWaitResult::TimedOut);
            self.enqueue_woken_task(*task_id);
        }
    }
}
