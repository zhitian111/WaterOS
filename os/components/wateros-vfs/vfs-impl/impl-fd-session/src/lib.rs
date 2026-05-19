//! per-task fd 表与控制台/pipe [`VfsIoHandle`] 实现。

#![no_std]

extern crate alloc;

pub mod handles;
pub mod registry;

pub use handles::{ConsoleInHandle, ConsoleOutHandle, PipeReadHandle, PipeWriteHandle};
pub use registry::PerTaskFdRegistry;
