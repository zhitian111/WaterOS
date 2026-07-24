//! 将 `log::Log` 桥接到 `runtime-console`：按级别着色并前缀 `[WaterOS]`。
//!
//! 仅由 crate 根 [`crate::init`] 调用 `init`；模块本身不对外公开。
//!
//! **不变量**：注册 logger 后 `log::max_level` 已设置，`to_level()` 在此路径下非 `Off`；否则 `unwrap` 会 panic。

use console::println;
use core::result::Result;
use log::{info, Level, Metadata, Record, SetLoggerError};

/// 静态全局 logger 实现，无字段；与 `LOGGER` 静态实例配合满足 `log::set_logger` 要求。
struct WaterOSLogger;
static LOGGER : WaterOSLogger = WaterOSLogger;

#[inline]
fn current_cpu_label() -> usize {
    #[cfg(feature = "impl-platform-console")]
    { return platform::arch::cpu::current_cpu_id().raw(); }
    #[cfg(not(feature = "impl-platform-console"))]
    { 0 }
}

impl log::Log for WaterOSLogger {
    #[inline]
    #[allow(unused)]
    fn enabled(&self, metadata : &Metadata) -> bool {
        if metadata.target()
                   .starts_with("ext4_rs") &&
           metadata.level() >= Level::Info
        {
            return false;
        }
        // `Off` 时无对应 `Level`，此处与 `log` 在已注册 logger 下的状态一致。
        metadata.level() <=
        log::max_level().to_level()
                        .unwrap()
    }

    #[inline]
    #[allow(unused)]
    fn log(&self, record : &Record) {
        if self.enabled(record.metadata()) {
            use console::AnsiColor;
            let color = match record.level() {
                Level::Error => AnsiColor::Red,
                Level::Warn => AnsiColor::Yellow,
                Level::Info => AnsiColor::Green,
                Level::Debug => AnsiColor::Blue,
                Level::Trace => AnsiColor::White,
            };

            println!("{}[WaterOS][cpu={}]{}{}    [{}]  {}{}",
                     AnsiColor::Cyan,
                     current_cpu_label(),
                     AnsiColor::Clear,
                     color,
                     record.level(),
                     record.args(),
                     AnsiColor::Clear);
        }
    }
    #[inline]
    #[allow(unused)]
    // 控制台为逐条写出，无独立 flush 通道；保留空实现以满足 trait。
    fn flush(&self) {}
}

/// 注册静态 [`WaterOSLogger`] 并设置 `log` 全局最大级别。
#[inline]
#[allow(unused)]
pub fn init(level : log::LevelFilter) -> Result<(), SetLoggerError> {
    log::set_logger(&LOGGER)?;
    log::set_max_level(level);
    info!("Logger initialized with level {:?}",
          log::max_level());
    Ok(())
}
