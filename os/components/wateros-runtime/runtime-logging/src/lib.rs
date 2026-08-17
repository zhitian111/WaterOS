#![no_std]
//! 将 `log` crate 接到 `runtime-console`：按 feature 锁定编译期最大级别，并由内部 logger 着色输出到控制台。
//!
//! 重导出的 `trace!` / `debug!` 等级别宏与 `log` 语义一致；须先调用 [`init`] 注册全局 logger。
//!
//! **构建约定**：`impl-trace` … `impl-error` 中至多启用一个。该 feature 会转发到
//! `log/max_level_*`，使更详细的日志宏及其参数求值在编译期被裁掉。
//!
//! RUNTIME_ORDER: 必须在 platform console 已可写后调用；`log::set_logger` 是全局
//! 一次性注册，重复调用不会替换已安装 logger。

mod logger;

#[cfg(any(all(feature = "impl-trace",
              any(feature = "impl-debug",
                  feature = "impl-info",
                  feature = "impl-warn",
                  feature = "impl-error")),
          all(feature = "impl-debug",
              any(feature = "impl-info", feature = "impl-warn", feature = "impl-error")),
          all(feature = "impl-info",
              any(feature = "impl-warn", feature = "impl-error")),
          all(feature = "impl-warn", feature = "impl-error"),))]
compile_error!("`impl-trace` ... `impl-error` are mutually exclusive; select one compile-time \
                log level");

/// 根据编译 feature（`impl-trace` … `impl-error`）注册全局 logger。
///
/// 初始化时只把 `log` 的运行时过滤器设置为相同的编译期上限，之后不再修改。
/// **契约**：可重复调用时行为以 `log::set_logger` 为准（第二次通常失败）；成功后会打印一条 Info 表示已初始化。
/// 返回值被故意忽略以保持旧启动 API；调用方应确保 BSP 单次调用，而不是用重试掩盖
/// 初始化顺序错误。
pub fn init() {
    use log::LevelFilter;
    #[cfg(any(feature = "impl-trace",
              feature = "impl-debug",
              feature = "impl-info",
              feature = "impl-warn",
              feature = "impl-error"))]
    let level = log::STATIC_MAX_LEVEL;
    #[cfg(not(any(feature = "impl-trace",
                  feature = "impl-debug",
                  feature = "impl-info",
                  feature = "impl-warn",
                  feature = "impl-error")))]
    let level = LevelFilter::Off;
    if level != LevelFilter::Off {
        let _ = logger::init(level);
    }
}

pub use log::debug;
pub use log::error;
pub use log::info;
pub use log::trace;
pub use log::warn;
