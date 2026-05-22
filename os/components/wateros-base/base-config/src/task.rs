//! 调度与任务子系统相关配置常量。

/// 每个任务在被 Tick 抢占前可连续运行的逻辑 tick 数。
///
/// 实际时间片 = `MAX_TICKS_PER_TASK` × 定时器间隔（当前为 100ms/tick）。
/// 增大此值可使调度行为更接近 FCFS。
pub const MAX_TICKS_PER_TASK : u64 = 100;
