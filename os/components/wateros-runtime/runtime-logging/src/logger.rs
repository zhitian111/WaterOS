//! 将 `log::Log` 桥接到 `runtime-console`：按级别着色并前缀 `[WaterOS]`。
//!
//! 仅由 crate 根 [`crate::init`] 调用 `init`；模块本身不对外公开。
//!
//! **不变量**：只有编译期最大级别非 `Off` 时才注册 logger，因此
//! `log::STATIC_MAX_LEVEL.to_level()` 在此路径下不会返回 `None`。

use console::println;
use core::result::Result;
use log::{info, Level, Metadata, Record, SetLoggerError};

/// 静态全局 logger 实现，无字段；与 `LOGGER` 静态实例配合满足 `log::set_logger` 要求。
///
/// OUTPUT_SYNC: 该对象不持有锁，所有串行化下沉到 runtime/platform console；`log()`
/// 内不得分配或再次调用 `log`，否则会在 allocator/console 路径上递归。
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
        // 编译期上限为常量；这里不再读取运行时原子过滤器。
        metadata.level() <=
        log::STATIC_MAX_LEVEL.to_level()
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

            // 一条 `println!` 对应 console 的一次整段写入，避免字段被不同 CPU 插入。
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

/// 注册静态 [`WaterOSLogger`] 并把 `log` 运行时过滤器初始化为编译期最大级别。
///
/// RUNTIME_ORDER: 成功注册后才设 max level，避免在无 logger 时放行记录；初始化后不再
/// 改变级别。失败时保持既有 logger 和 max level 原样不变。
#[inline]
#[allow(unused)]
pub fn init(level : log::LevelFilter) -> Result<(), SetLoggerError> {
    log::set_logger(&LOGGER)?;
    log::set_max_level(level);
    info!("Logger initialized with level {:?}",
          log::max_level());
    Ok(())
}
