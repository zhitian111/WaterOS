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
    // Cargo feature 合并可能同时打开多个级别；取最安静一档，避免误开 trace 淹没 QEMU 日志。
    let level = {
        #[cfg(feature = "impl-error")]
        {
            LevelFilter::Error
        }
        #[cfg(all(not(feature = "impl-error"), feature = "impl-warn"))]
        {
            LevelFilter::Warn
        }
        #[cfg(all(not(feature = "impl-error"), not(feature = "impl-warn"), feature = "impl-info"))]
        {
            LevelFilter::Info
        }
        #[cfg(all(
            not(feature = "impl-error"),
            not(feature = "impl-warn"),
            not(feature = "impl-info"),
            feature = "impl-debug"
        ))]
        {
            LevelFilter::Debug
        }
        #[cfg(all(
            not(feature = "impl-error"),
            not(feature = "impl-warn"),
            not(feature = "impl-info"),
            not(feature = "impl-debug"),
            feature = "impl-trace"
        ))]
        {
            LevelFilter::Trace
        }
        #[cfg(all(
            not(feature = "impl-error"),
            not(feature = "impl-warn"),
            not(feature = "impl-info"),
            not(feature = "impl-debug"),
            not(feature = "impl-trace")
        ))]
        {
            LevelFilter::Off
        }
    };
    if level != LevelFilter::Off {
        let _ = logger::init(level);
    }
}

pub use log::debug;
pub use log::error;
pub use log::info;
pub use log::trace;
pub use log::warn;
