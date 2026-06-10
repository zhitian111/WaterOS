//! `SCHED_FIFO` 就绪队列算法（不含完整 [`Scheduler`] trait）。

#![no_std]

extern crate alloc;

mod queue;

pub use queue::RtFifoRunQueue;
