//! task相关


use crate::TaskTrapSnapshot;
/// 任务在系统内的唯一标识。
pub type TaskId = usize;
/// 调度器使用的逻辑时钟单位。
pub type TaskTick = u64;
/// 任务退出时返回给上层的状态码。
pub type TaskExitCode = isize;
/// 等待队列在调度器中的唯一标识。
pub type WaitQueueId = usize;


/// 区分内核任务与用户态任务；影响 TCB 是否持有用户栈与地址空间句柄等资源路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskKind {
    /// 只在内核态运行的任务。
    Kernel,
    /// 拥有用户栈与用户返回现场的用户态任务
    User,
}

/// 调度器驱动的任务生命周期状态；与就绪队列/阻塞队列表示需保持一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskState {
    /// 已就绪，等待被调度运行。
    Ready,
    /// 当前正在 CPU 上运行。
    Running,
    /// 由于某种阻塞原因暂时不可运行（等待目标见 [`TaskWaitTarget`]）。
    Blocking(crate::TaskWaitTarget),
    /// 睡眠到指定 tick 后再尝试唤醒。
    Sleeping { wake_tick : TaskTick },
    /// 已退出，不会再被调度。
    Exited(TaskExitCode),
}

/// 调度器为任务维护的基础运行统计。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskRuntimeStats {
    /// 该任务累计被切入运行的次数。
    pub schedule_count : usize,
    /// 该任务累计消耗的 tick 数。
    pub tick_count : usize,
}


/// 已退出任务的可回收信息。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitedTask {
    /// 任务号。
    pub id : TaskId,
    /// 父任务号；无父任务时为 `None`。
    pub parent_id : Option<TaskId>,
    /// 任务类别。
    pub kind : TaskKind,
    /// 退出状态码。
    pub exit_code : TaskExitCode,
    /// 退出前最后一次 trap 语义快照。
    pub trap_frame : Option<TaskTrapSnapshot>,
    /// 退出时刻的运行统计。
    pub stats : TaskRuntimeStats,
}
