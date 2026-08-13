#![no_std]
//! 内核运行时聚合层：将 panic、控制台、日志与堆分配等子 crate 以统一模块名再导出，供 `wateros` 根 crate 按能力选用。
//!
//! 各子能力的行为与平台实现由对应子 crate 的 feature 决定；本文件不包含独立逻辑。
//!
//! **边界**：本 crate 只做再导出，不引入新的类型或初始化顺序；根 crate 负责按引导顺序调用各子模块的 `init` / `panic_handler` 等入口。
//!
//! RUNTIME_ORDER: 推荐顺序是 arch/platform early init → console 可写 → logging init →
//! heap init → 可能分配内存的 driver、VFS、task。panic handler 可在最早阶段安装，
//! 但早期输出只能是 best-effort。

// 以下子模块与依赖 crate 同名，便于 `wateros` 使用 `runtime::console` 等形式引用。

/// 再导出 panic 终止路径，供根 crate 挂接 `#[panic_handler]`。
pub mod panic {
    pub use panic::panic_handler;
}
/// 再导出控制台 API 与宏（`print!` / `println!` 等）。
pub mod console {
    pub use console::*;
}

/// 确保正式启动日志前已经触发控制台后端；此阶段不得依赖 heap 或 logger。
pub fn init_console() {
    console::write_raw_bytes(&[]);
}

/// 打印启动横幅；调用者应在 [`init_console`] 之后调用。
pub fn showlogo() {
    console::show_logo();
}
/// 再导出 `log` 宏与 [`logging::init`]。
pub mod logging {
    pub use logging::*;
}

/// 再导出堆初始化、统计和分配错误处理。
pub mod heap_allocator {
    pub use heap_allocator::*;
}

#[cfg(feature = "self_test")]
/// runtime 组件统一自检入口。
pub fn self_test() {
    logging::info!("[runtime] self_test begin");
    heap_allocator::self_test();
    logging::info!("[runtime] self_test complete");
}

#[cfg(feature = "serial-uart-virt")]
pub mod serial {
    pub use ::runtime_serial::*;
}
