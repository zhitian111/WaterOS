//! 对外可见的 trap 与任务快照类型
use crate::{CpuId, CpuMask, SchedPolicy, TaskId, TaskKind, TaskRuntimeStats, TaskState, VRunTime};

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
/// 语义字段（id/state/policy/stats/affinity 等）之外，还携带调度器做“当前任务
/// 粗粒度查询”所需的实现级字段：内核栈栈顶与地址空间 token。它们只在内核内部
/// 传递，不面向用户态，用于让 `current_task_snapshot` 一次拿锁取全。
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
    pub policy : SchedPolicy,
    /// `sched_priority`；三个 fair 策略（OTHER/BATCH/IDLE）下恒为 0。
    pub priority : i32,
    /// fair 策略（OTHER/BATCH/IDLE）的 nice 值，范围为 -20 到 19。
    ///
    /// 该值用于 tick 时计算 vruntime 增量；FIFO/RR 保存该字段但不以它决定实时优先级。
    pub nice : i8,
    /// Linux `ioprio` 原始编码（class 位于高 3 位，data 位于低 13 位）。
    ///
    /// 该属性属于线程，fork/clone 时继承；0 表示 `IOPRIO_CLASS_NONE`。
    pub io_priority : u16,
    /// fair 策略的累计虚拟运行时间；TCB 是唯一真相。
    pub vruntime : VRunTime,
    /// 最近一次 trap 的语义快照。
    pub trap_frame : Option<TaskTrapSnapshot>,
    /// 调度器维护的运行统计。
    pub stats : TaskRuntimeStats,
    /// 任务当前所属的就绪 CPU。
    pub ready_cpu_id : Option<CpuId>,
    /// 任务当前运行的 CPU。
    pub running_cpu_id : Option<CpuId>,
    /// 任务上次运行的 CPU。
    pub last_cpu_id : Option<CpuId>,
    /// CPU 亲和性掩码。
    pub affinity : CpuMask,
    /// 用户地址空间指针（内核任务为 0）。
    pub user_aspace_ptr : usize,
    /// 用户地址空间 token（satp）；0 = 内核地址空间。
    pub user_address_space_token : usize,
    /// trap 返回时应恢复的地址空间 token。
    pub trap_return_address_space_token : usize,
    /// 内核栈栈顶。
    pub kernel_stack_top : usize,
    /// 任务上下文指针（用于 __switch）。
    pub task_cx : *const (),
}
