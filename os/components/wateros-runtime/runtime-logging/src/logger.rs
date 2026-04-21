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

#[inline]
#[allow(unused)]
pub fn init(level : log::LevelFilter) -> Result<(), SetLoggerError> {
    log::set_logger(&LOGGER)?;
    log::set_max_level(level);
    info!("Logger initialized with level {:?}",
          log::max_level());
    Ok(())
}
