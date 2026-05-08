#![no_std]
//! 将 `log` crate 接到 `runtime-console`：按 feature 设置最大级别，并由内部 logger 着色输出到控制台。
//!
//! 重导出的 `trace!` / `debug!` 等级别宏与 `log` 语义一致；须先调用 [`init`] 注册全局 logger。
//!
//! **构建约定**：应只启用一个 `impl-trace` … `impl-error` 级别 feature；若多个同时启用，下面各 `cfg` 块会依次执行，可能导致多次 `log::set_logger`（通常仅首次成功）。

mod logger;

/// 根据编译 feature（`impl-trace` … `impl-error`）注册全局 logger 与 `log::max_level`。
///
/// **契约**：可重复调用时行为以 `log::set_logger` 为准（第二次通常失败）；成功后会打印一条 Info 表示已初始化。
pub fn init() {
    use log::LevelFilter;
    // 与 `Cargo.toml` 中互斥的级别 feature 对应；默认 `impl-trace` 见该文件。
    #[cfg(feature = "impl-trace")]
    let _ = logger::init(LevelFilter::Trace);
    #[cfg(feature = "impl-debug")]
    let _ = logger::init(LevelFilter::Debug);
    #[cfg(feature = "impl-info")]
    let _ = logger::init(LevelFilter::Info);
    #[cfg(feature = "impl-warn")]
    let _ = logger::init(LevelFilter::Warn);
    #[cfg(feature = "impl-error")]
    let _ = logger::init(LevelFilter::Error);
}

pub use log::debug;
pub use log::error;
pub use log::info;
pub use log::trace;
pub use log::warn;
