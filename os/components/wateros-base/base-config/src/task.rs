//! 调度与任务子系统相关配置常量。

/// 监督态定时器重武装间隔（毫秒），与 [`trap_handler`] 及 clock syscall 的 sleep 换算一致。
///
/// [`trap_handler`]: 组合层内核 trap 路由（`os/src/trap_handler.rs`）。
pub const SCHED_TIMER_PERIOD_MS : u64 = 10;

/// 每个任务在被 Tick 抢占前可连续运行的逻辑 tick 数。
///
/// 实际时间片 = `MAX_TICKS_PER_TASK` × 定时器间隔（当前为 10ms/tick）。
/// 增大此值可使调度行为更接近 FCFS。
pub const MAX_TICKS_PER_TASK : u64 = 10;

/// 每个 `SCHED_RR` 任务在被 Tick 抢占前可连续运行的逻辑 tick 数。
///
/// 实际时间片 = `MAX_RT_TICKS_PER_TASK` × 定时器间隔（当前为 10ms/tick）。
pub const MAX_RT_TICKS_PER_TASK : u64 = 10;
