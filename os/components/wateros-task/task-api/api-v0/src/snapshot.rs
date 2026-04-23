use crate::{
    TaskId, TaskKind, TaskRuntimeStats, TaskState, TaskTrapFrame, TaskExitCode, UserTaskResources,
};

/// 对外暴露的稳定任务快照。
///
/// 这里故意不暴露内核栈地址、bootstrap 协议细节和保存上下文布局，
/// 让公共 API 更偏语义，而不是直接泄漏实现形状。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskSnapshot {
    /// 任务号。
    pub id: TaskId,
    /// 任务类别。
    pub kind: TaskKind,
    /// 当前任务状态。
    pub state: TaskState,
    /// 最近一次 trap 的保存现场。
    pub trap_frame: Option<TaskTrapFrame>,
    /// 调度器维护的运行统计。
    pub stats: TaskRuntimeStats,
    /// 若为用户任务，则附带其资源快照。
    pub user_resources: Option<UserTaskResources>,
}

/// 已退出任务的可回收信息。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitedTask {
    /// 任务号。
    pub id: TaskId,
    /// 任务类别。
    pub kind: TaskKind,
    /// 退出状态码。
    pub exit_code: TaskExitCode,
    /// 退出前最后一次 trap 现场。
    pub trap_frame: Option<TaskTrapFrame>,
    /// 退出时刻的运行统计。
    pub stats: TaskRuntimeStats,
    /// 若为用户任务，则附带其退出时的资源快照。
    pub user_resources: Option<UserTaskResources>,
}
