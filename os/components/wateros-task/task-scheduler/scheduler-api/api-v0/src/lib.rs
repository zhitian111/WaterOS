//! 调度器侧 **trait 与调度原因** 抽象：描述实现必须提供的操作集合，与
//! `task_api` 中的任务类型配合使用。
//!
//! 具体轮转、优先级等算法在 `scheduler-impl` 中实现本模块的
//! [`Scheduler`]；**不** 定义单任务内存表示（见 `wateros-task-impl-core`）。

#![no_std]

extern crate alloc;

mod cfs_queue;
mod cpu;
mod fifo_queue;
mod registry;
mod rr_queue;
mod wait_queues;
pub use cpu::*;
pub use registry::TaskRegistry;
pub use wait_queues::WaitQueues;
