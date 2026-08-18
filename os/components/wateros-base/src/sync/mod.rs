//! 内核全局状态使用的多核互斥和一次发布容器。
//! 互斥容器适合短临界区；一次发布容器适合初始化后只读的全局状态。

mod multiprocessor;
mod once;

pub use multiprocessor::MultiprocessorSafeCell;
pub use once::{BootOnceCell, OnceInitError, RuntimeOnceCell};
