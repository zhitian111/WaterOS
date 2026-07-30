//! 调度与任务子系统相关配置常量。

/// 当前内核静态支持的最大逻辑 CPU 数。
///
/// 这是静态容量上限，不表示 configured 或 online CPU 数量。
pub const MAX_CPUS : usize = 32;

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
/// 内核任务栈大小（字节）。
pub const KERNEL_TASK_STACK_SIZE : usize = 32 * 1024;

/// Linux/CFS 兼容的 `nice -20..=19` 到调度权重映射。
///
/// 下标为 `nice + 20`。权重越大，SCHED_OTHER 任务相同实际运行时间累积的
/// vruntime 越少；实时调度类不得使用本表决定 FIFO/RR 优先级。
pub const NICE_TO_WEIGHT : [u64; 40] = [
    88761, 71755, 56483, 46273, 36291,  // -20 ~ -16
    29154, 23254, 18705, 14949, 11916,  // -15 ~ -11
    9548, 7620, 6100, 4904, 3906,       // -10 ~ -6
    3121, 2501, 1991, 1586, 1277,       // -5 ~ -1
    1024, 820, 655, 526, 423,           // 0 ~ 4 (0 = 1024)
    335, 272, 215, 172, 137,            // 5 ~ 9
    110, 87, 70, 56, 45,                // 10 ~ 14
    36, 29, 23, 18, 15,                 // 15 ~ 19
];

/// `nice = 0` 的基准权重，用于把实际运行时间换算为 vruntime。
pub const NICE_0_WEIGHT : u64 = 1024;
