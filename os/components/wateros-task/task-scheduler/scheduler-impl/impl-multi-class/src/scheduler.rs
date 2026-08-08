//! [`MultiClassScheduler`]：OTHER/BATCH/IDLE/FIFO/RR 五类调度。

extern crate alloc;
use alloc::boxed::Box;
use alloc::vec::Vec;
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

use api_v0::{RescheduleCause, ScheduleReason};

unsafe extern "C" {
    static kernel_heap_start: u8;
    static kernel_heap_end: u8;
}

pub(super) struct MultiClassScheduler {
    pub registry : TaskRegistry,
    pub wait_queues : WaitQueues,
    pub cpu_states : Box<[CPUState]>,
    /// 环形选核的起点。负载相同时，从这里开始的第一个 online CPU 获胜。
    pub next_placement_cpu : usize,
    /// 唯一推进全局 sleep/wait timeout 时间的 BSP。
    pub timekeeper_cpu : Option<CpuId>,
    /// 入队时在 scheduler 锁内累计，锁外再实际发送定向 IPI。
    pub pending_reschedule_cpus : CpuMask,
    /// 被强制迁出本 CPU 的当前任务。在 `__switch` 保存完上下文前，
    /// 目标 CPU 不能在 runqueue 中看到它。
    deferred_ready_after_switch : [Option<TaskId>; MAX_CPUS],
}

/// Ready 任务的放置偏好；最终目标仍须满足 online 与 affinity 约束。
#[derive(Clone, Copy)]
pub(super) enum ReadyPlacement {
    LeastLoaded,
    LastCpu,
    Prefer(CpuId),
}

impl MultiClassScheduler {
    // ================================================================
    //  构造与初始化
    // ================================================================
    pub(super) fn new() -> Self {
        let mut cpu_states = Vec::with_capacity(MAX_CPUS);
        for raw in 0..MAX_CPUS {
            cpu_states.push(CPUState::new(CpuId::from_raw(raw)));
        }
        Self { registry : TaskRegistry::new(),
               wait_queues : WaitQueues::new(),
               cpu_states : cpu_states.into_boxed_slice(),
               next_placement_cpu : 0,
               timekeeper_cpu : None,
               pending_reschedule_cpus : CpuMask::EMPTY,
               deferred_ready_after_switch : [None; MAX_CPUS] }
    }

    pub(super) fn init(&mut self, boot_cpu : CpuId) {
        self.registry.init();
        self.wait_queues
            .init();
        self.next_placement_cpu = 0;
        self.timekeeper_cpu = None;
        self.pending_reschedule_cpus = CpuMask::EMPTY;
        self.deferred_ready_after_switch = [None; MAX_CPUS];
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
            let idle_snapshot = self.registry
                                    .task_snapshot(idle_id);
            cpu_state.set_current_task(&idle_snapshot);
        }
    }

    // ================================================================
    //  核心调度
    // ================================================================

    /// 标记任务为 Running 并更新当前 CPU 的 current_task_id。
    /// CACHE_SYNC: TaskSnapshot → CPUState;  
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
            panic!("[scheduler] corrupted switch target: cpu={} task={} cx={:#x} ra={:#x} \
                    sp={:#x} heap=[{:#x},{:#x})",
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
        self.cpu_states[cpu_id.raw()].leave_boot_context();
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
            // The caller has already consumed `need_resched`. Rechecking with
            // Tick semantics here would discard a Forced request.
            ScheduleReason::Reschedule => {}
            ScheduleReason::Tick => self.tick(cpu_id)?,
            // Yield / Block / Sleep / Exit：在选下一个任务之前确保所有到期任务已入队
            _ => {
                if self.is_timekeeper_cpu(cpu_id) {
                    self.activate_woken_and_timeout_tasks();
                }
            }
        }

        // Phase 3: 从 cpu_states 取出当前任务
        let current_task_id = self.cpu_states[cpu_id.raw()].current_task_id()
                                                           .expect("current task must exist");
        let current_ptr = self.cpu_states[cpu_id.raw()].current_task_cx;

        // Phase 4: IDLE 特殊处理（不经过 enqueue）
        if self.cpu_states[cpu_id.raw()].is_current_idle() {
            let next_task_id = self.cpu_states[cpu_id.raw()].pick_next_runnable();
            let snap = self.registry
                           .task_snapshot(next_task_id);
            if next_task_id == current_task_id {
                self.set_current_task(&snap, cpu_id);
                return None;
            }
            self.set_current_task(&snap, cpu_id);
            let next_ptr = snap.task_cx as *const TaskContext;
            return Some(self.switch_pair(current_ptr,
                                         next_task_id,
                                         next_ptr,
                                         cpu_id));
        }
        // Phase 5-8: 非 IDLE 调度
        if matches!(reason,
                    ScheduleReason::Yield | ScheduleReason::Sleep(0))
        {
            self.cpu_states[cpu_id.raw()].prepare_yield();
        }
        let queue_target = self.pick_queue(reason);
        self.enqueue_task(queue_target, current_task_id, cpu_id);
        // 当前任务的状态转换可能唤醒其它任务（最典型是 Exit 唤醒父 runner）。
        // 必须在转换之后取 next；提前 pick 会错误地选 idle，并令 CPU cache 与
        // 实际将要恢复的任务脱节。本地无任务可跑时顺势尝试从其它核偷取。
        let next_task_id = self.cpu_states[cpu_id.raw()].pick_next_runnable();
        let snap = self.registry
                       .task_snapshot(next_task_id);
        // Phase 8: 决定是否 __switch
        let is_exit = matches!(reason, ScheduleReason::Exit(_));
        if next_task_id == current_task_id {
            if is_exit {
                panic!("[scheduler] exited task {} remained runnable on CPU {}",
                       current_task_id,
                       cpu_id.raw());
            }
            self.set_current_task(&snap, cpu_id);
            return None;
        }
        self.set_current_task(&snap, cpu_id);
        let next_ptr = snap.task_cx as *const TaskContext;
        Some(self.switch_pair(current_ptr,
                              next_task_id,
                              next_ptr,
                              cpu_id))
    }


    /// Tick 前置处理：推进时间、检查时间片与抢占条件。
    fn tick(&mut self, cpu_id : CpuId) -> Option<()> {
        // 1. 推进全局 tick
        if self.is_timekeeper_cpu(cpu_id) {
            self.wait_queues
                .tick();
        }
        // 2. 仅推进 CPU 本地缓存；任务统计会在离开 CPU 时统一回写 TCB。
        // 3. 推进当前任务的时间片/vruntime（由 CPUState::tick 按策略分发）
        self.cpu_states[cpu_id.raw()].tick();

        let needs_switch =
            self.cpu_states[cpu_id.raw()].cpu_should_reschedule(RescheduleCause::Tick);

        // 5. 处理唤醒/超时任务
        if needs_switch ||
           self.wait_queues
               .has_woken_or_timeout_tasks()
        {
            if self.is_timekeeper_cpu(cpu_id) {
                self.activate_woken_and_timeout_tasks();
            }
        }
        if needs_switch {
            Some(())
        } else {
            None
        }
    }
    fn pick_queue(&mut self, reason : ScheduleReason) -> QueueTarget {
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
            self.activate_woken_and_timeout_tasks();
        }

        // ===== Phase 2: 快速路径 — 目标已就绪，无需阻塞 =====
        if self.registry
               .wait_target_ready(target)
        {
            if let Some(current_task_id) = self.cpu_states[cpu_id.raw()].current_task_id() {
                self.registry
                    .finish_wait(current_task_id, TaskWaitResult::Woken);
            }
            return None;
        }

        // ===== Phase 3: 从 cpu_states 取出当前任务 =====
        let current_task_id = self.cpu_states[cpu_id.raw()].current_task_id()?;
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
        Some(self.switch_pair(current_ptr,
                              next_task_id,
                              next_ptr,
                              cpu_id))
    }

    /// TCB_SYNC: sync_current_to_registry → Registry 写回
    ///
    /// 这里只回写当前 CPU cache；目标 runqueue 的 vruntime 归一化只能在
    /// `enqueue_ready_on_cpu` 中按目标 CPU 的 baseline 完成。
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
        if CPUState::is_cfs_policy(policy) {
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
            // Yield/Tick 后优先留在本核；affinity 不允许时才回退到最小负载 CPU。
            QueueTarget::Ready => {
                let affinity = self.registry
                                   .get_affinity(current_task_id)
                                   .expect("current task must exist");
                if affinity.contains(cpu_id) {
                    self.activate_ready_task(current_task_id,
                                             ReadyPlacement::Prefer(cpu_id));
                } else {
                    let slot = &mut self.deferred_ready_after_switch[cpu_id.raw()];
                    assert!(slot.is_none(),
                            "CPU {} already has a deferred task migration",
                            cpu_id.raw());
                    *slot = Some(current_task_id);
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
                    self.activate_ready_task(*waiter_id, ReadyPlacement::LastCpu);
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
                        self.activate_ready_task(*waiter_id, ReadyPlacement::LastCpu);
                    }
                }
                self.wait_queues
                    .enqueue_exited_task(current_task_id);
                self.registry
                    .mark_exited(current_task_id, exit_code);
            }
        }
    }

    /// 在源 CPU 已经物理保存完离开任务的寄存器和内核栈后，再将它
    /// 发布到 affinity 允许的目标 runqueue。
    pub(super) fn complete_context_switch(&mut self, cpu_id : CpuId) {
        let Some(task_id) = self.deferred_ready_after_switch[cpu_id.raw()].take() else {
            return;
        };
        self.activate_ready_task(task_id, ReadyPlacement::LeastLoaded);
    }
    /// 激活非当前任务：选核、入 ready queue，并按统一 CPU 抢占规则请求调度。
    pub(super) fn activate_ready_task(&mut self,
                                      task_id : TaskId,
                                      placement : ReadyPlacement)
                                      -> CpuId {
        let target = self.pick_ready_cpu(task_id, placement);
        self.enqueue_ready_on_cpu(task_id, target);
        let policy = self.registry
                         .task_snapshot(task_id)
                         .policy;
        self.request_reschedule(target, RescheduleCause::Ready(policy));
        target
    }

    fn pick_ready_cpu(&mut self, task_id : TaskId, placement : ReadyPlacement) -> CpuId {
        let snap = self.registry
                       .task_snapshot(task_id);
        let preferred = match placement {
            ReadyPlacement::LeastLoaded => None,
            // 唤醒亲和性放宽：last_cpu 过载时，把任务放到更空的核。
            ReadyPlacement::LastCpu => snap.last_cpu_id
                                           .filter(|cpu_id| !self.cpu_is_overloaded(*cpu_id)),
            ReadyPlacement::Prefer(cpu_id) => Some(cpu_id),
        };
        if let Some(cpu_id) = preferred.filter(|cpu_id| {
                                           cpu_id.fits_capacity(self.cpu_states
                                                                    .len())
                                       })
                                       .filter(|cpu_id| self.cpu_states[cpu_id.raw()].online)
                                       .filter(|cpu_id| {
                                           snap.affinity
                                               .contains(*cpu_id)
                                       })
        {
            return cpu_id;
        }

        // 从环形起点开始选择负载最小的可用 CPU，避免相同负载长期偏向 CPU 0。
        let mut best_cpu = None;
        let mut min_load = usize::MAX;
        for offset in 0..self.cpu_states
                             .len()
        {
            let index = (self.next_placement_cpu + offset) %
                        self.cpu_states
                            .len();
            let cpu_id = CpuId::from_raw(index);
            if !self.cpu_states[index].online ||
               !snap.affinity
                    .contains(cpu_id)
            {
                continue;
            }
            let load = self.cpu_load(cpu_id);
            if load < min_load {
                min_load = load;
                best_cpu = Some(cpu_id);
            }
        }
        let cpu_id = best_cpu.expect("cannot enqueue a task without an online CPU");
        self.next_placement_cpu = (cpu_id.raw() + 1) %
                                  self.cpu_states
                                      .len();
        cpu_id
    }

    /// 根据 CPU 的统一抢占规则记录一次异步重调度请求。
    pub(super) fn request_reschedule(&mut self, cpu_id : CpuId, cause : RescheduleCause) {
        if !self.cpu_states[cpu_id.raw()].cpu_should_reschedule(cause) {
            return;
        }
        self.mark_need_resched(cpu_id);
    }

    /// `cpu_should_reschedule()` 已经判断为真时，只记录请求，不再重复判断。
    fn mark_need_resched(&mut self, cpu_id : CpuId) {
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
    /// TCB_SYNC: mark_ready → Registry,vruntime 归一化 → Registry
    /// TCB → 目标 CPU ready queue 的唯一入口。
    /// 只修改 TCB 与 runqueue，不产生 reschedule/IPI 请求。
    fn enqueue_ready_on_cpu(&mut self, task_id : TaskId, cpu_id : CpuId) {
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
            // 包括同 CPU 重复入队在内，先清掉旧归属，确保一个任务只在一个
            // ready queue 中出现一次。
            self.cpu_states[old_cpu_id.raw()].dequeue(task_id);
        }
        let mut snap = self.registry
                           .task_snapshot(task_id);
        if let Some(vruntime) =
            self.cpu_states[cpu_id.raw()].normalize_vruntime(snap.vruntime, snap.policy)
        {
            snap.vruntime = vruntime;
            self.registry
                .set_vruntime(task_id, vruntime);
        }
        self.registry
            .mark_ready(task_id, cpu_id);
        self.cpu_states[cpu_id.raw()].enqueue(task_id, &snap);
    }
    /// 从所有 CPU 的就绪队列摘除任务（用于 kill / discard 等跨 CPU 操作）。
    fn dequeue_from_all_cpus(&mut self, task_id : TaskId) {
        for cpu in &mut self.cpu_states {
            cpu.dequeue(task_id);
        }
    }
    /// 到期睡眠/超时任务到就绪队列。(超时唤醒)
    fn activate_woken_and_timeout_tasks(&mut self) {
        for task_id in &self.wait_queues
                            .woken_tasks()
        {
            self.activate_ready_task(*task_id, ReadyPlacement::LastCpu);
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
            self.activate_ready_task(*task_id, ReadyPlacement::LastCpu);
        }
    }
}
