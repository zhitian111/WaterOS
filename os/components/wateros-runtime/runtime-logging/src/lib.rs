#![no_std]

mod logger;

pub fn init() {
    use log::LevelFilter;
    #[cfg(feature = "impl-trace")]
    logger::init(LevelFilter::Trace);
    #[cfg(feature = "impl-debug")]
    logger::init(LevelFilter::Debug);
    #[cfg(feature = "impl-info")]
    logger::init(LevelFilter::Info);
    #[cfg(feature = "impl-warn")]
    logger::init(LevelFilter::Warn);
    #[cfg(feature = "impl-error")]
    logger::init(LevelFilter::Error);
}

pub use log::debug;
pub use log::error;
pub use log::info;
pub use log::trace;
pub use log::warn;
