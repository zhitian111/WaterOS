//! 调度与任务子系统相关配置常量。

/// 监督态定时器重武装间隔（毫秒），与 [`trap_handler`] 及 clock syscall 的 sleep 换算一致。
///
/// [`trap_handler`]: 组合层内核 trap 路由（`os/src/trap_handler.rs`）。
pub const SCHED_TIMER_PERIOD_MS : u64 = 10;

/// 每个任务在被 Tick 抢占前可连续运行的逻辑 tick 数。
///
/// 实际时间片 = `MAX_TICKS_PER_TASK` × 定时器间隔（当前为 10ms/tick）。
/// 增大此值可使调度行为更接近 FCFS，并减少 lmbench 等微基准在测量窗口内被
/// 非自愿 tick 抢占（lat_ctx 非法样本 → score=0）。
pub const MAX_TICKS_PER_TASK : u64 = 50;

/// `pick_next` 连续跳过多少 stale ready 条目后触发一次 lazy compact。
///
/// 高 churn 场景（如 lat_ctx fork N 进程 pipe 乒乓）下 stale 会膨胀 pick 成本；
/// compact 仅删除已失效条目，不改变调度语义。
pub const READY_QUEUE_STALE_COMPACT_THRESHOLD : usize = 8;

/// 每个 `SCHED_RR` 任务在被 Tick 抢占前可连续运行的逻辑 tick 数。
///
/// 实际时间片 = `MAX_RT_TICKS_PER_TASK` × 定时器间隔（当前为 10ms/tick）。
pub const MAX_RT_TICKS_PER_TASK : u64 = 10;
/// 内核任务栈大小（字节）。
pub const KERNEL_TASK_STACK_SIZE : usize = 32 * 1024;
