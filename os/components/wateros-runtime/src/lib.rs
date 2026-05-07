#![no_std]
//! 内核运行时聚合层：将 panic、控制台、日志与堆分配等子 crate 以统一模块名再导出，供 `wateros` 根 crate 按能力选用。
//!
//! 各子能力的行为与平台实现由对应子 crate 的 feature 决定；本文件不包含独立逻辑。

pub mod panic {
    pub use panic::panic_handler;
}
pub mod console {
    pub use console::*;
}
pub mod logging {
    pub use logging::*;
}

pub mod heap_allocator {
    pub use heap_allocator::*;
}
