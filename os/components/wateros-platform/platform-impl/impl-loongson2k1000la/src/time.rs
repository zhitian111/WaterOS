//! 平台 timebase 频率占位：任务 09 用 CPUCFG 4/5 推导。

use api_v0::time::{PlatformTime, PlatformTimeError, PlatformTimeResult};

pub struct Loongson2K1000Time;

impl PlatformTime for Loongson2K1000Time {
    fn get_time_frequency_hz() -> PlatformTimeResult<u64> {
        Err(PlatformTimeError::Unsupported)
    }
}

pub use Loongson2K1000Time as PlatformTimeImpl;
