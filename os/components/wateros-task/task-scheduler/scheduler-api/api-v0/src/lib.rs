//! 调度器侧 **trait 与调度原因** 抽象：描述实现必须提供的操作集合，与
//! `task_api` 中的任务类型配合使用。
//!
//! 具体轮转、优先级等算法在 `scheduler-impl` 中实现本模块的
//! [`Scheduler`]；**不** 定义单任务内存表示（见 `wateros-task-impl-core`）。

#![no_std]

extern crate alloc;

mod registry;
mod wait_queues;

use task_api::{TaskExitCode, TaskTick, TaskWaitTarget};


pub use registry::TaskRegistry;
pub use task_api::SchedulableCheck;
pub use wait_queues::WaitQueues;

/// 首次上下文切换所需的指针对（bootstrap/current → next）。
pub type SwitchPair =
    (*mut arch::task::ActiveArchTaskContext, *const arch::task::ActiveArchTaskContext);

/// 一次调度决策的触发来源；由 `RoundRobinScheduler::schedule`
/// 等解释为就绪/阻塞/睡眠队列目标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleReason {
    /// 第一次切入任务系统（当前实现中主要由 `prepare_first_switch` 路径覆盖）。
    StartFirst,
    /// 当前任务主动让出 CPU。
    Yield,
    /// 由时钟 tick 触发一次调度检查
    Tick,
    /// 由于阻塞而切换出去。
    Block(TaskWaitTarget),
    /// 由于定时睡眠而切换出去；`ticks == 0` 时在实现中等价于 yield。
    Sleep(TaskTick),
    /// 当前任务退出。
    Exit(TaskExitCode),
}
/// 将当前任务从运行态移出后应进入的调度桶。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueTarget {
    /// 进入就绪队列（具体由 active_impl 的 run-queue 决定）。
    Ready,
    /// 阻塞等待（等待目标见 [`TaskWaitTarget`]）。
    Blocked(TaskWaitTarget),
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
