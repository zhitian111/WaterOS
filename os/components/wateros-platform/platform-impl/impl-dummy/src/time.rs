//! 不可用的占位时间频率源。

use api_v0::time::{PlatformTime, PlatformTimeError, PlatformTimeResult};

pub struct PlatformDummyTime;

impl PlatformTime for PlatformDummyTime {
    #[inline]
    fn get_time_frequency_hz() -> PlatformTimeResult<u64> { Err(PlatformTimeError::Unsupported) }
}

pub use PlatformDummyTime as PlatformTimeImpl;
