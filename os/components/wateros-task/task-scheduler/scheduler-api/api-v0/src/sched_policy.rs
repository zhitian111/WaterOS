//! 多调度类共享的契约类型：run-queue 目标、可调度性查询与策略变更动作。

use task_api::{TaskBlockReason, TaskExitCode, TaskTick};

/// 将当前任务从运行态移出后应进入的调度桶。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueTarget {
    /// 进入就绪队列（具体由 active_impl 的 run-queue 决定）。
    Ready,
    /// 阻塞等待。
    Blocked(TaskBlockReason),
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
