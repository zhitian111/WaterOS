use crate::cfs_queue::CfsQueue;
use crate::fifo_queue::FifoQueue;
use crate::rr_queue::RrQueue;
use crate::{registry, TaskRegistry, WaitQueues};
use arch::task::{ActiveArchTaskContext, ArchTaskContext};
use config::task::{MAX_TICKS_PER_TASK, NICE_0_WEIGHT, NICE_TO_WEIGHT};
use task_api::{
    AddressSpaceHandle, CpuId, CpuMask, Nice, Priority, SchedError, SchedPolicy, TaskExitCode,
    TaskId, TaskSnapshot, TaskTick, TaskWaitTarget, VRunTime, NICE_MIN, PRIORITY_MIN,
};
// 用固定点表示每 tick 的 vruntime，避免高权重（负 nice）任务因整数除法得到 0。
const VRUNTIME_SCALE : u64 = 1 << 20;
pub type SwitchPair =
    (*mut arch::task::ActiveArchTaskContext, *const arch::task::ActiveArchTaskContext);

/// 一次调度决策的触发来源；由 `RoundRobinScheduler::schedule`
/// 等解释为就绪/阻塞/睡眠队列目标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleReason {
    /// 第一次切入任务系统（当前实现中主要由 `prepare_first_switch` 路径覆盖）。
    StartFirst,
    /// 当前任务主动让出 CPU。
    Yield,
    /// IPI 或本地入队请求的重调度检查；不应无条件让出当前任务。
    Reschedule,
    /// 由时钟 tick 触发一次调度检查
    Tick,
    /// 由于阻塞而切换出去。
    Block(TaskWaitTarget),
    /// 由于定时睡眠而切换出去；`ticks == 0` 时在实现中等价于 yield。
    Sleep(TaskTick),
    /// 当前任务退出。
    Exit(TaskExitCode),
}

/// CPU 本地重调度判断的触发来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RescheduleCause {
    /// timer tick 或已设置 `need_resched` 后的常规调度检查。
    Tick,
    /// 一个指定 policy 的任务刚进入本 CPU ready queue。
    Ready(SchedPolicy),
    /// 当前任务已不允许继续运行，必须立刻重新调度。
    Forced,
}
/// 将当前任务从运行态移出后应进入的调度桶。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueTarget {
    /// 进入就绪队列（具体由 active_impl 的 run-queue 决定）。
    Ready,
    /// 阻塞等待（等待目标见 [`TaskWaitTarget`]）。
    Blocked(TaskWaitTarget),
    /// 睡眠至指定逻辑 tick。
    Sleeping(TaskTick),
    /// 已退出。
    Exited(TaskExitCode),
}

pub struct CPUState {
    pub cpu_id : CpuId,
    pub boot_task_cx : ActiveArchTaskContext,
    /// `run_first_task` 前 CPU 仍在启动栈上；此时 current cache 预置为 idle，
    /// 但尚不能据此校验运行中的 `sp`。
    pub boot_context_active : bool,
    pub current_task_id : Option<TaskId>,
    pub idle_task_id : Option<TaskId>,
    pub online : bool,
    /// 有新任务进入本 CPU 队列，需在安全点重新判断是否抢占。
    pub need_resched : bool,
    /// 本 CPU 实际切入另一任务（包括首次切入 idle）的次数。
    pub context_switches : u64,
    /// 本 CPU 已处理的 scheduler timer tick 次数。
    pub timer_ticks : u64,
    /// 本 CPU 当前任务为 idle 时累计的 scheduler timer tick 次数。
    pub idle_ticks : u64,
    pub min_vruntime : u64,
    pub idle_min_vruntime : u64,
    pub cfs_queue : CfsQueue,
    pub rr_queue : RrQueue,
    pub fifo_queue : FifoQueue,
    pub batch_queue : CfsQueue,
    pub idle_queue : CfsQueue,
    //当前任务运行的tick数
    pub current_ticks : u64,
    /// 当前任务自上次切入或同步以来实际运行的 tick 数，用于批量回写 TCB 统计。
    pub current_runtime_ticks : u64,
    pub current_policy : SchedPolicy,
    pub current_priority : Priority,
    pub current_vruntime : VRunTime,
    pub current_nice : Nice,
    pub current_affinity : CpuMask,
    /// 缓存当前任务的上下文指针，避免每次调度都查 registry。
    pub current_task_cx : *mut ActiveArchTaskContext,
    /// 缓存当前任务的用户地址空间指针，用于快速判断是否需要切换页表。
    pub current_aspace : usize,
}
impl CPUState {
    pub fn new(cpu_id : CpuId) -> Self {
        Self { cpu_id,
               boot_task_cx : ActiveArchTaskContext::zero_init(),
               boot_context_active : true,
               current_task_id : None,
               idle_task_id : None,
               online : false,
               need_resched : false,
               context_switches : 0,
               timer_ticks : 0,
               idle_ticks : 0,
               min_vruntime : 0,
               idle_min_vruntime : 0,
               cfs_queue : CfsQueue::new(),
               rr_queue : RrQueue::new(),
               fifo_queue : FifoQueue::new(),
               batch_queue : CfsQueue::new(),
               idle_queue : CfsQueue::new(),
               current_ticks : 0,
               current_runtime_ticks : 0,
               current_policy : SchedPolicy::Other,
               current_priority : PRIORITY_MIN,
               current_vruntime : 0,
               current_nice : NICE_MIN,
               current_affinity : CpuMask::EMPTY,
               current_task_cx : core::ptr::null_mut(),
               current_aspace : 0 }
    }
    pub fn init(&mut self, cpu_id : CpuId) {
        self.cpu_id = cpu_id;
        self.boot_task_cx = ActiveArchTaskContext::zero_init();
        self.boot_context_active = true;
        self.current_task_id = None;
        self.idle_task_id = None;
        self.online = false;
        self.need_resched = false;
        self.context_switches = 0;
        self.timer_ticks = 0;
        self.idle_ticks = 0;
        self.min_vruntime = 0;
        self.idle_min_vruntime = 0;
        self.cfs_queue
            .init();
        self.rr_queue.init();
        self.fifo_queue
            .init();
        self.batch_queue
            .init();
        self.idle_queue
            .init();
        self.current_ticks = 0;
        self.current_runtime_ticks = 0;
        self.current_policy = SchedPolicy::Other;
        self.current_priority = PRIORITY_MIN;
        self.current_vruntime = 0;
        self.current_nice = NICE_MIN;
        self.current_affinity = CpuMask::EMPTY;
        self.current_task_cx = core::ptr::null_mut();
        self.current_aspace = 0;
    }
    /// OTHER 与 BATCH 共用的公平调度基线。
    pub fn min_vruntime(&self) -> VRunTime { self.min_vruntime }
    pub fn idle_min_vruntime(&self) -> VRunTime { self.idle_min_vruntime }

    /// 使用 CFS vruntime 队列的 policy。
    ///
    /// `Idle` 使用独立 baseline，但仍由 CFS tree 保存并按 vruntime 排序。
    pub const fn is_cfs_policy(policy : SchedPolicy) -> bool {
        matches!(policy,
                 SchedPolicy::Other | SchedPolicy::Batch | SchedPolicy::Idle)
    }

    pub fn boot_task_cx(&mut self) -> *mut ActiveArchTaskContext {
        &mut self.boot_task_cx as *mut ActiveArchTaskContext
    }
    pub fn normalize_vruntime(&self,
                              vruntime : VRunTime,
                              policy : SchedPolicy)
                              -> Option<VRunTime> {
        match policy {
            SchedPolicy::Other | SchedPolicy::Batch => Some(vruntime.max(self.min_vruntime)),
            SchedPolicy::Idle => Some(vruntime.max(self.idle_min_vruntime)),
            _ => None,
        }
    }

    fn min_ready_fair_vruntime(&self) -> Option<VRunTime> {
        self.cfs_queue
            .min_ready_vruntime()
            .into_iter()
            .chain(self.batch_queue
                       .min_ready_vruntime())
            .min()
    }
    fn min_idle_ready_vruntime(&self) -> Option<VRunTime> {
        self.idle_queue
            .min_ready_vruntime()
    }
    /// 当前运行 fair 实体和两个 fair ready tree 的左端共同、单调地推进基线。
    fn update_min_vruntime(&mut self) {
        let mut candidate = self.min_ready_fair_vruntime();
        if !self.is_current_idle() &&
           matches!(self.current_policy,
                    SchedPolicy::Other | SchedPolicy::Batch)
        {
            candidate = Some(candidate.map_or(self.current_vruntime, |ready| {
                                          ready.min(self.current_vruntime)
                                      }));
        }
        if let Some(candidate) = candidate {
            self.min_vruntime = self.min_vruntime
                                    .max(candidate);
        }
    }
    fn update_idle_min_vruntime(&mut self) {
        let mut candidate = self.min_idle_ready_vruntime();
        if !self.is_current_idle() && matches!(self.current_policy, SchedPolicy::Idle) {
            candidate = Some(candidate.map_or(self.current_vruntime, |ready| {
                                          ready.min(self.current_vruntime)
                                      }));
        }
        if let Some(candidate) = candidate {
            self.idle_min_vruntime = self.idle_min_vruntime
                                         .max(candidate);
        }
    }
    pub fn leave_boot_context(&mut self) { self.boot_context_active = false; }
    pub fn set_online(&mut self, online : bool) { self.online = online; }
    pub fn online(&self) -> bool { self.online }
    pub fn set_idle_task_id(&mut self, task_id : TaskId) { self.idle_task_id = Some(task_id); }
    pub fn tick(&mut self) {
        self.timer_ticks = self.timer_ticks
                               .saturating_add(1);
        if self.is_current_idle() {
            self.idle_ticks = self.idle_ticks
                                  .saturating_add(1);
        }
        self.current_runtime_ticks = self.current_runtime_ticks
                                         .saturating_add(1);
        match self.current_policy {
            SchedPolicy::Other => {
                // 物理 idle task 不属于 CFS；它运行时不能推进普通任务的
                // vruntime 基线，否则新唤醒任务会被无端惩罚。
                if !self.is_current_idle() {
                    let weight = NICE_TO_WEIGHT[(self.current_nice + 20) as usize];
                    let delta = NICE_0_WEIGHT.saturating_mul(VRUNTIME_SCALE)
                                             .saturating_div(weight)
                                             .max(1);
                    self.current_vruntime = self.current_vruntime
                                                .saturating_add(delta);
                    self.update_min_vruntime();
                }
            }
            SchedPolicy::Batch => {
                if !self.is_current_idle() {
                    let weight = NICE_TO_WEIGHT[(self.current_nice + 20) as usize];
                    let delta = NICE_0_WEIGHT.saturating_mul(VRUNTIME_SCALE)
                                             .saturating_div(weight)
                                             .max(1);
                    self.current_vruntime = self.current_vruntime
                                                .saturating_add(delta);
                    self.update_min_vruntime();
                }
            }
            SchedPolicy::Idle => {
                if !self.is_current_idle() {
                    let weight = NICE_TO_WEIGHT[(self.current_nice + 20) as usize];
                    let delta = NICE_0_WEIGHT.saturating_mul(VRUNTIME_SCALE)
                                             .saturating_div(weight)
                                             .max(1);
                    self.current_vruntime = self.current_vruntime
                                                .saturating_add(delta);
                    self.update_idle_min_vruntime();
                }
            }
            SchedPolicy::Rr => {
                self.current_ticks = self.current_ticks
                                         .saturating_add(1);
            }
            SchedPolicy::Fifo => {}
        }
    }
    /// 就绪队列中最高实时优先级（FIFO/RR，不含 OTHER）。
    pub fn highest_priority(&self) -> Option<Priority> {
        self.fifo_queue
            .highest_priority()
            .into_iter()
            .chain(self.rr_queue
                       .highest_priority())
            .max()
    }

    /// 统一调度判断：给定触发来源时，当前任务是否应让出 CPU。
    pub fn cpu_should_reschedule(&self, cause : RescheduleCause) -> bool {
        if !self.current_affinity
                .contains(self.cpu_id)
        {
            return true;
        }

        match cause {
            RescheduleCause::Forced => return true,
            // 弱唤醒策略只唤醒物理 idle CPU；繁忙 CPU 在下一 tick 再比较
            // 相应队列的 vruntime。
            RescheduleCause::Ready(SchedPolicy::Batch)
                if !self.is_current_idle() && self.current_policy != SchedPolicy::Idle =>
            {
                return false;
            }
            RescheduleCause::Ready(SchedPolicy::Idle) if !self.is_current_idle() => {
                return false;
            }
            RescheduleCause::Tick | RescheduleCause::Ready(_) => {}
        }


        let is_idle = self.current_task_id == self.idle_task_id;
        if is_idle {
            if self.min_ready_fair_vruntime()
                   .is_some() ||
               self.highest_priority()
                   .is_some() ||
               self.min_idle_ready_vruntime()
                   .is_some()
            {
                return true;
            }
            return false;
        }

        match self.current_policy {
            SchedPolicy::Other => {
                if self.highest_priority()
                       .is_some()
                {
                    return true;
                }
                self.min_ready_fair_vruntime()
                    .is_some_and(|min| self.current_vruntime > min)
            }
            SchedPolicy::Batch => {
                self.highest_priority()
                    .is_some() ||
                self.min_ready_fair_vruntime()
                    .is_some_and(|min| self.current_vruntime > min)
            }
            SchedPolicy::Idle => {
                self.highest_priority()
                    .is_some() ||
                self.cfs_queue
                    .task_count() >
                0 ||
                self.batch_queue
                    .task_count() >
                0 ||
                self.min_idle_ready_vruntime()
                    .is_some_and(|min| self.current_vruntime > min)
            }
            SchedPolicy::Fifo => {
                if self.highest_priority()
                       .is_some_and(|p| p > self.current_priority)
                {
                    return true;
                }
                false
            }
            SchedPolicy::Rr => {
                if self.current_ticks >= MAX_TICKS_PER_TASK {
                    return true;
                }
                if self.highest_priority()
                       .is_some_and(|p| p > self.current_priority)
                {
                    return true;
                }
                false
            }
        }
    }

    /// 按优先级从就绪队列中选择下一个可运行任务。
    pub fn pick_next_runnable(&mut self) -> TaskId {
        // FIFO → RR，按优先级 99→1 穿插扫描。
        for priority in (1..=99).rev() {
            if let Some(task_id) = self.fifo_queue
                                       .pick_at_priority(priority)
            {
                return task_id;
            }
            if let Some(task_id) = self.rr_queue
                                       .pick_at_priority(priority)
            {
                return task_id;
            }
        }
        // OTHER 与 BATCH 属于同一 fair class，按跨队列的最小 vruntime 选择。
        // 相同 vruntime 时优先 Other，形成轻微的 Batch 劣后而不会饿死 Batch。
        let other_min = self.cfs_queue
                            .min_ready_vruntime();
        let batch_min = self.batch_queue
                            .min_ready_vruntime();
        let picked = match (other_min, batch_min) {
            (Some(_), None) => self.cfs_queue
                                   .pick(),
            (None, Some(_)) => self.batch_queue
                                   .pick(),
            (Some(other), Some(batch)) if other <= batch => self.cfs_queue
                                                                .pick(),
            (Some(_), Some(_)) => self.batch_queue
                                      .pick(),
            (None, None) => None,
        };
        if let Some((task_id, vruntime)) = picked {
            self.min_vruntime = self.min_vruntime
                                    .max(vruntime);
            return task_id;
        }
        if let Some((task_id, vruntime)) = self.idle_queue
                                               .pick()
        {
            self.idle_min_vruntime = self.idle_min_vruntime
                                         .max(vruntime);
            return task_id;
        }
        self.idle_task_id
            .expect("every CPU must have an idle task")
    }

    /// 当前任务是否为 idle 任务。
    pub fn is_current_idle(&self) -> bool { self.current_task_id == self.idle_task_id }

    pub fn current_task_id(&self) -> Option<TaskId> { self.current_task_id }

    pub fn set_current_task_id(&mut self, task_id : TaskId) {
        self.current_task_id = Some(task_id);
    }
    /// 本 CPU 所有队列中的可运行任务总数。
    pub fn load(&self) -> usize {
        let current = self.current_task_id != self.idle_task_id;
        self.rr_queue
            .task_count() +
        self.fifo_queue
            .task_count() +
        self.cfs_queue
            .task_count() +
        self.batch_queue
            .task_count() +
        self.idle_queue
            .task_count() +
        current as usize
    }

    /// 从本 CPU 所有队列中摘除任务。
    pub fn dequeue(&mut self, task_id : TaskId) {
        self.cfs_queue
            .dequeue(task_id);
        self.batch_queue
            .dequeue(task_id);
        self.fifo_queue
            .dequeue(task_id);
        self.rr_queue
            .dequeue(task_id);
        self.idle_queue
            .dequeue(task_id);
    }

    /// 按策略将任务入队到对应的就绪队列。
    pub fn enqueue(&mut self, task_id : TaskId, snap : &TaskSnapshot) {
        match snap.policy {
            SchedPolicy::Other => self.cfs_queue
                                      .enqueue(task_id, snap.vruntime),
            SchedPolicy::Batch => self.batch_queue
                                      .enqueue(task_id, snap.vruntime),
            SchedPolicy::Idle => self.idle_queue
                                     .enqueue(task_id, snap.vruntime),
            SchedPolicy::Fifo => self.fifo_queue
                                     .enqueue(task_id, snap.priority),
            SchedPolicy::Rr => self.rr_queue
                                   .enqueue(task_id, snap.priority),
        }
    }

    /// 用快照更新所有 CPUState 缓存；aspace 切换由 scheduler 层处理。
    pub fn set_current_task(&mut self, snap : &TaskSnapshot) {
        if self.current_task_id != Some(snap.id) {
            self.context_switches = self.context_switches
                                        .saturating_add(1);
        }
        self.current_task_id = Some(snap.id);
        self.current_policy = snap.policy;
        self.current_priority = snap.priority;
        self.current_vruntime = snap.vruntime;
        self.current_nice = snap.nice;
        self.current_ticks = 0;
        self.current_runtime_ticks = 0;
        self.current_affinity = snap.affinity;
        self.current_task_cx = snap.task_cx as *mut ActiveArchTaskContext;
        self.current_aspace = snap.user_aspace_ptr;
    }

    /// Policy 参数与本 CPU 调度队列的对应关系。
    ///
    /// 这是关联函数而非实例方法：校验不依赖具体 CPU 状态，但新增/删除队列时
    /// 可与 `enqueue()` 的 policy 分派一起维护。
    pub fn validate_policy_param(policy : SchedPolicy,
                                 priority : Priority)
                                 -> Result<(), SchedError> {
        match policy {
            SchedPolicy::Fifo | SchedPolicy::Rr if !(1..=99).contains(&priority) => {
                Err(SchedError::InvalidArg)
            }
            policy if Self::is_cfs_policy(policy) && priority != 0 => Err(SchedError::InvalidArg),
            _ => Ok(()),
        }
    }
}
pub struct CpuSnapshot {
    pub cpu_id : CpuId,
    pub online : bool,
    pub current_task_id : Option<TaskId>,
    pub idle_task_id : Option<TaskId>,
    pub current_address_space : Option<AddressSpaceHandle>,
    pub current_is_idle : bool,
    pub current_is_user : bool,
    /// 四类队列中等待运行的任务数，不含当前正在运行的任务。
    pub runnable_other : usize,
    pub runnable_batch : usize,
    pub runnable_fifo : usize,
    pub runnable_rr : usize,
    pub runnable_idle : usize,
    pub need_resched : bool,
    pub context_switches : u64,
    pub timer_ticks : u64,
    pub idle_ticks : u64,
    pub current_ticks : u64,
}
