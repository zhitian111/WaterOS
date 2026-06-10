//! Linux 调度策略与 CPU 亲和性相关的 **语义类型**（不含 syscall 号与用户拷贝）。

/// Linux `sched_*` 策略编号（bring-up 子集）。
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedPolicy {
    /// 默认分时策略（`SCHED_OTHER`）；当前 WaterOS 轮转调度对用户态呈现为此类。
    Other = 0,
    /// 实时 FIFO（`SCHED_FIFO`）；bring-up 未实现。
    Fifo = 1,
    /// 实时 RR（`SCHED_RR`）；bring-up 未实现。
    Rr = 2,
}

impl SchedPolicy {
    /// 由 Linux 原始 policy 值解析；未知值返回 `None`。
    #[must_use]
    pub const fn from_linux_raw(raw: i32) -> Option<Self> {
        match raw {
            0 => Some(Self::Other),
            1 => Some(Self::Fifo),
            2 => Some(Self::Rr),
            _ => None,
        }
    }

    /// 当前 bring-up 下内核实际提供的有效策略。
    #[must_use]
    pub const fn effective_for_bringup() -> Self {
        Self::Other
    }
}

/// `struct sched_param` 中与 bring-up 相关的字段。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SchedParam {
    /// `sched_priority`；`SCHED_OTHER` 下恒为 0。
    pub priority: i32,
}

/// 调度/亲和性操作错误（由聚合层映射为 Linux errno）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedError {
    /// 非法参数（如负 pid、缓冲区过小、非法 policy 值）。
    InvalidArg,
    /// 目标 task/tid 不存在。
    NoSuchTask,
    /// 无权限或策略不受支持（如 RT 策略、改 affinity）。
    NotPermitted,
}

/// 查询任务是否仍在就绪选取路径上可见（由 [`TaskRegistry`] 等实现）。
pub trait SchedulableCheck {
    /// 任务是否可被选中运行。
    fn is_schedulable(&self, task_id: crate::TaskId) -> bool;
}

/// lp64 下返回给 userspace 的有效 CPU mask 字节数。
pub const SCHED_CPU_MASK_RET_BYTES: usize = 8;

/// 查询 affinity 时用户缓冲区的最小长度。
pub const SCHED_CPU_MASK_MIN_BYTES: usize = 8;
