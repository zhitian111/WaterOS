//! 平台 timebase 频率占位：任务 05 从 OpenSBI/DTB 探测。

use api_v0::time::{PlatformTime, PlatformTimeError, PlatformTimeResult};

pub struct JH7110Time;

impl PlatformTime for JH7110Time {
    fn get_time_frequency_hz() -> PlatformTimeResult<u64> {
        Err(PlatformTimeError::Unsupported)
    }
}

pub use JH7110Time as PlatformTimeImpl;
