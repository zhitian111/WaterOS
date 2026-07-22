use crate::{
    queues::{FifoQueue, OtherQueue, RrQueue},
    registry, TaskRegistry, WaitQueues,
};
use arch::task::{ActiveArchTaskContext, ArchTaskContext};
use task_api::{AddressSpaceHandle, CpuId, TaskExitCode, TaskId, TaskTick, TaskWaitTarget};
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

/// `set_scheduler` 完成后调度器应执行的动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchedPolicyChangeAction {
    /// 无需立即重新调度。
    NoReschedule,
    /// 应立即抢占并切换到更高优先级任务。
    RescheduleNow,
}

pub struct CPUScheduler {
    pub cpu_id : CpuId,
    pub boot_task_cx : ActiveArchTaskContext,
    pub current_task_id : Option<TaskId>,
    pub idle_task_id : Option<TaskId>,
    pub current_task_ticks : u64,
    pub online : bool,
    /// 有新任务进入本 CPU 队列，需在安全点重新判断是否抢占。
    pub need_resched : bool,
    /// 本 CPU 实际切入另一任务（包括首次切入 idle）的次数。
    pub context_switches : u64,
    /// 本 CPU 已处理的 scheduler timer tick 次数。
    pub timer_ticks : u64,
    pub other_queue : OtherQueue,
    pub rr_queue : RrQueue,
    pub fifo_queue : FifoQueue,
}
impl CPUScheduler {
    pub fn new(cpu_id : CpuId) -> Self {
        Self { cpu_id,
               boot_task_cx : ActiveArchTaskContext::zero_init(),
               current_task_id : None,
               idle_task_id : None,
               current_task_ticks : 0,
               online : false,
               need_resched : false,
               context_switches : 0,
               timer_ticks : 0,
               other_queue : OtherQueue::new(),
               rr_queue : RrQueue::new(),
               fifo_queue : FifoQueue::new() }
    }
    pub fn current_task_id(&self) -> Option<TaskId> { self.current_task_id }
    pub fn boot_task_cx(&mut self) -> *mut ActiveArchTaskContext {
        &mut self.boot_task_cx as *mut ActiveArchTaskContext
    }
}

pub struct GlobalScheduler {
    pub registry : TaskRegistry,
    pub wait_queues : WaitQueues,
}
impl GlobalScheduler {
    pub fn new() -> Self {
        Self { registry : TaskRegistry::new(),
               wait_queues : WaitQueues::new() }
    }
    pub fn init(&mut self) {
        self.registry.init();
        self.wait_queues
            .init();
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
    /// 三类队列中等待运行的任务数，不含当前正在运行的任务。
    pub runnable_other : usize,
    pub runnable_fifo : usize,
    pub runnable_rr : usize,
    pub need_resched : bool,
    pub context_switches : u64,
    pub timer_ticks : u64,
    pub current_task_ticks : u64,
}
