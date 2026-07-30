//! 内核全局状态使用的多核互斥和一次发布容器。

mod multiprocessor;
mod once;

pub use multiprocessor::MultiprocessorSafeCell;
pub use once::{BootOnceCell, OnceInitError, RuntimeOnceCell};
