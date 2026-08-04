#![no_std]
//! 将 `log` crate 接到 `runtime-console`：按 feature 设置最大级别，并由内部 logger 着色输出到控制台。
//!
//! 重导出的 `trace!` / `debug!` 等级别宏与 `log` 语义一致；须先调用 [`init`] 注册全局 logger。
//!
//! **构建约定**：启用 `impl-trace` … `impl-error` 中的级别 feature；若多个同时启用，取等级最低（最详细）的一档。
//!
//! RUNTIME_ORDER: 必须在 platform console 已可写后调用；`log::set_logger` 是全局
//! 一次性注册，重复调用不会替换已安装 logger。

mod logger;

/// 根据编译 feature（`impl-trace` … `impl-error`）注册全局 logger 与 `log::max_level`。
///
/// **契约**：可重复调用时行为以 `log::set_logger` 为准（第二次通常失败）；成功后会打印一条 Info 表示已初始化。
/// 返回值被故意忽略以保持旧启动 API；调用方应确保 BSP 单次调用，而不是用重试掩盖
/// 初始化顺序错误。
pub fn init() {
    use log::LevelFilter;
    // Cargo feature 合并可能同时打开多个级别；取等级最低（最详细）一档，例如 error+warn → warn，info+trace → trace。
    let level = {
        #[cfg(feature = "impl-trace")]
        {
            LevelFilter::Trace
        }
        #[cfg(all(not(feature = "impl-trace"), feature = "impl-debug"))]
        {
            LevelFilter::Debug
        }
        #[cfg(all(
            not(feature = "impl-trace"),
            not(feature = "impl-debug"),
            feature = "impl-info"
        ))]
        {
            LevelFilter::Info
        }
        #[cfg(all(
            not(feature = "impl-trace"),
            not(feature = "impl-debug"),
            not(feature = "impl-info"),
            feature = "impl-warn"
        ))]
        {
            LevelFilter::Warn
        }
        #[cfg(all(
            not(feature = "impl-trace"),
            not(feature = "impl-debug"),
            not(feature = "impl-info"),
            not(feature = "impl-warn"),
            feature = "impl-error"
        ))]
        {
            LevelFilter::Error
        }
        #[cfg(all(
            not(feature = "impl-trace"),
            not(feature = "impl-debug"),
            not(feature = "impl-info"),
            not(feature = "impl-warn"),
            not(feature = "impl-error")
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
pub use log::LevelFilter;

/// Change the runtime filter after the logger has been installed. Operator
/// mode uses this to keep an interactive prompt readable without rebuilding.
pub fn set_max_level(level: LevelFilter) { log::set_max_level(level); }
