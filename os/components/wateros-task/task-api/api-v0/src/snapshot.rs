//! 对外可见的 trap 与任务快照类型
use crate::{SchedPolicy, TaskId, TaskKind, TaskRuntimeStats, TaskState};

/// 对外暴露的 trap 语义快照。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskTrapSnapshot {
    /// 原始 trap cause 编码，具体解释由当前架构约定。
    pub raw_cause : usize,
    /// trap 发生或恢复时关联的用户态 PC。
    pub user_pc : usize,
    /// trap 发生或恢复时关联的用户态 SP。
    pub user_sp : usize,
    /// trap 发生或恢复时关联的用户态地址空间指针。
    pub user_aspace_ptr : usize,
    /// fault 地址或架构提供的附加 trap 值。
    pub fault_addr : usize,
    /// 该现场恢复时是否会返回用户态。
    pub returns_to_user : bool,
}

impl TaskTrapSnapshot {
    /// 构造一份架构无关的 trap 语义快照。
    #[inline]
    pub const fn new(raw_cause : usize,
                     user_pc : usize,
                     user_sp : usize,
                     user_aspace_ptr : usize,
                     fault_addr : usize,
                     returns_to_user : bool)
                     -> Self {
        Self { raw_cause,
               user_pc,
               user_sp,
               user_aspace_ptr,
               fault_addr,
               returns_to_user }
    }

    /// 返回原始 trap cause 编码。
    #[inline]
    pub const fn raw_cause(&self) -> usize { self.raw_cause }

    /// 返回 trap 关联的用户态 PC。
    #[inline]
    pub const fn user_pc(&self) -> usize { self.user_pc }

    /// 返回 trap 关联的用户态 SP。
    #[inline]
    pub const fn user_sp(&self) -> usize { self.user_sp }

    /// 返回 fault 地址或架构提供的附加 trap 值。
    #[inline]
    pub const fn fault_addr(&self) -> usize { self.fault_addr }

    /// 判断该现场恢复时是否会返回用户态。
    #[inline]
    pub const fn returns_to_user(&self) -> bool { self.returns_to_user }

    /// 判断该现场恢复时是否会返回内核态。
    #[inline]
    pub const fn returns_to_kernel(&self) -> bool { !self.returns_to_user }

    /// 返回 trap 关联的用户页表对象指针。
    pub const fn user_aspace_ptr(&self) -> usize { self.user_aspace_ptr }
}

/// 对外暴露的稳定任务快照。
///
/// 这里故意不暴露内核栈地址、bootstrap 协议细节和保存上下文布局，
/// 让公共 API 更偏语义，而不是直接泄漏实现形状。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskSnapshot {
    /// 任务号。
    pub id : TaskId,
    /// 父任务号；无父任务时为 `None`。
    pub parent_id : Option<TaskId>,
    /// 任务类别。
    pub kind : TaskKind,
    /// 当前任务状态。
    pub state : TaskState,
    /// 有效调度策略。
    pub sched_policy : SchedPolicy,
    /// `sched_priority`；`SCHED_OTHER` 下恒为 0。
    pub sched_priority : i32,
    /// `SCHED_OTHER` 的 nice 值，范围为 -20 到 19。
    ///
    /// 当前仅保存该属性；普通任务队列的加权公平选择将在后续接入。
    pub nice : i8,
    /// 最近一次 trap 的语义快照。
    pub trap_frame : Option<TaskTrapSnapshot>,
    /// 调度器维护的运行统计。
    pub stats : TaskRuntimeStats,
}
