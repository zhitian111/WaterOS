//! Linux 调度策略与 CPU 亲和性相关的 **语义类型**（不含 syscall 号与用户拷贝）。

/// Linux `sched_*` 策略编号（bring-up 子集）。
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedPolicy {
    /// 默认分时公平策略（`SCHED_OTHER`）。
    Other = 0,
    /// 实时 FIFO（`SCHED_FIFO`）。
    Fifo = 1,
    /// 实时 RR（`SCHED_RR`）。
    Rr = 2,
    /// 面向 CPU 密集型后台工作的公平调度策略。
    Batch = 3,
    /// Linux `SCHED_IDLE`（raw policy 值为 5）。
    Idle = 5,
}

impl SchedPolicy {
    /// 由 Linux 原始 policy 值解析；未知值返回 `None`。
    #[must_use]
    pub const fn from_linux_raw(raw : i32) -> Option<Self> {
        match raw {
            0 => Some(Self::Other),
            1 => Some(Self::Fifo),
            2 => Some(Self::Rr),
            3 => Some(Self::Batch),
            5 => Some(Self::Idle),
            _ => None,
        }
    }

    /// 新建任务在未显式设置策略时采用的默认策略。
    #[must_use]
    pub const fn effective_for_bringup() -> Self { Self::Other }
}
/// `SCHED_OTHER` 的最低 nice 值（CPU 份额最高）。
pub const NICE_MIN : i8 = -20;
/// `SCHED_OTHER` 的最高 nice 值（CPU 份额最低）。
pub const NICE_MAX : i8 = 19;
pub type Nice = i8;
pub type VRunTime = u64;
pub type Priority = i32;
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
/// lp64 下返回给 userspace 的有效 CPU mask 字节数。
pub const SCHED_CPU_MASK_RET_BYTES : usize = 8;
/// 查询 affinity 时用户缓冲区的最小长度。
pub const SCHED_CPU_MASK_MIN_BYTES : usize = 8;
pub const PRIORITY_MIN : i32 = 1;
pub const PRIORITY_MAX : i32 = 99;
pub const BUCKET_COUNT : usize = (PRIORITY_MAX - PRIORITY_MIN + 1) as usize;
