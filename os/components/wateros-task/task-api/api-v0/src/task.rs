/// 任务在系统内的唯一标识。
pub type TaskId = usize;
/// 调度器使用的逻辑时钟单位。
pub type TaskTick = u64;
/// 任务退出时返回给上层的状态码。
pub type TaskExitCode = isize;
/// 等待队列在调度器中的唯一标识。
pub type WaitQueueId = usize;
/// 内核任务入口函数签名。
pub type KernelTaskEntry = extern "C" fn(usize) -> !;
/// 用户任务首次进入时的目标 PC。
pub type UserTaskEntryPc = usize;

/// 预留给 idle 任务的固定任务号。
pub const IDLE_TASK_ID: TaskId = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskKind {
    /// 只在内核态运行的任务。
    Kernel,
    /// 后续用于承载用户态任务。
    User,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskBlockReason {
    /// 主动让出 CPU。
    Yield,
    /// 因定时睡眠而阻塞。
    Sleep,
    /// 因等待某个可阻塞对象而休眠。
    Wait(crate::TaskWaitHandle),
    /// 因系统调用路径暂时挂起。
    UserSyscall,
    /// 由内核显式置为阻塞。
    Manual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskState {
    /// 已就绪，等待被调度运行。
    Ready,
    /// 当前正在 CPU 上运行。
    Running,
    /// 由于某种阻塞原因暂时不可运行。
    Blocking(TaskBlockReason),
    /// 睡眠到指定 tick 后再尝试唤醒。
    Sleeping { wake_tick: TaskTick },
    /// 已退出，不会再被调度。
    Exited(TaskExitCode),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleReason {
    /// 第一次切入任务系统。
    StartFirst,
    /// 当前任务主动让出 CPU。
    Yield,
    /// 由时钟 tick 触发一次调度检查。
    Tick,
    /// 由于阻塞而切换出去。
    Block(TaskBlockReason),
    /// 由于定时睡眠而切换出去。
    Sleep(TaskTick),
    /// 当前任务退出。
    Exit(TaskExitCode),
}

/// 调度器为任务维护的基础运行统计。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskRuntimeStats {
    /// 该任务累计被切入运行的次数。
    pub schedule_count: usize,
    /// 该任务累计消耗的 tick 数。
    pub tick_count: usize,
}
