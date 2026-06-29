//! 任务身份、状态机与运行统计：**调度与实现层共享的语义类型**，不绑定具体 TCB
//! 内存布局。
//!
//! 与 `snapshot`、`user`、`wait` 等模块共同构成
//! `task_api`；变更状态或阻塞原因枚举时需同步调度器与 `impl-core` 的解读路径。

use crate::TaskTrapSnapshot;
/// 任务在系统内的唯一标识。
pub type TaskId = usize;
/// 调度器使用的逻辑时钟单位。
pub type TaskTick = u64;
/// 任务退出时返回给上层的状态码。
pub type TaskExitCode = isize;
/// 等待队列在调度器中的唯一标识。
pub type WaitQueueId = usize;

/// 预留给 idle 任务的固定任务号。
pub const IDLE_TASK_ID : TaskId = 0;

/// 区分内核任务与用户态任务；影响 TCB 是否持有用户栈与地址空间句柄等资源路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskKind {
    /// 只在内核态运行的任务。
    Kernel,
    /// 拥有用户栈与用户返回现场的用户态任务（与
    /// `TaskControlBlock::new_user_task` 路径对应）。
    User,
}

/// 任务进入 `TaskState::Blocking` 时的原因；调度器据此分入不同等待结构。
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

/// 调度器驱动的任务生命周期状态；与就绪队列/阻塞队列表示需保持一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskState {
    /// 已就绪，等待被调度运行。
    Ready,
    /// 当前正在 CPU 上运行。
    Running,
    /// 由于某种阻塞原因暂时不可运行。
    Blocking(TaskBlockReason),
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
