#![no_std]
//! 内核运行时聚合层：将 panic、控制台、日志与堆分配等子 crate 以统一模块名再导出，供 `wateros` 根 crate 按能力选用。
//!
//! 各子能力的行为与平台实现由对应子 crate 的 feature 决定；本文件不包含独立逻辑。
//!
//! **边界**：本 crate 只做再导出，不引入新的类型或初始化顺序；根 crate 负责按引导顺序调用各子模块的 `init` / `panic_handler` 等入口。

// 以下子模块与依赖 crate 同名，便于 `wateros` 使用 `runtime::console` 等形式引用。

/// 再导出 [`panic::panic_handler`]，供根 crate 挂接 `#[panic_handler]`。
pub mod panic {
    pub use panic::panic_handler;
}
/// 再导出控制台 API 与宏（`print!` / `println!` 等）。
pub mod console {
    pub use console::*;
}
/// 再导出 `log` 宏与 [`logging::init`]。
pub mod logging {
    pub use logging::*;
}

/// 再导出堆初始化与分配错误处理（若启用伙伴分配器 feature）。
pub mod heap_allocator {
    pub use heap_allocator::*;
}

#[cfg(feature = "serial-uart-virt")]
pub mod serial {
    pub use ::runtime_serial::*;
}
