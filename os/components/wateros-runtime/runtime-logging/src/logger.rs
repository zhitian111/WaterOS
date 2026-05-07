//! 将 `log::Log` 桥接到 `runtime-console`：按级别着色并前缀 `[WaterOS]`。
//!
//! 仅由 crate 根 [`crate::init`] 调用 `init`；模块本身不对外公开。

use console::println;
use core::result::Result;
use log::{info, Level, Metadata, Record, SetLoggerError};
struct WaterOSLogger;
static LOGGER : WaterOSLogger = WaterOSLogger;

impl log::Log for WaterOSLogger {
    #[inline]
    #[allow(unused)]
    fn enabled(&self, metadata : &Metadata) -> bool {
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

            println!("{}[WaterOS]{}{}    [{}]  {}{}",
                     AnsiColor::Cyan,
                     AnsiColor::Clear,
                     color,
                     record.level(),
                     record.args(),
                     AnsiColor::Clear);
        }
    }
    #[inline]
    #[allow(unused)]
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
