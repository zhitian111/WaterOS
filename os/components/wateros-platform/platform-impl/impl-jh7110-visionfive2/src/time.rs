use api_v0::time::{PlatformTime, PlatformTimeResult};

pub struct VisionFive2Time;

impl PlatformTime for VisionFive2Time {
    fn get_time_frequency_hz() -> PlatformTimeResult<u64> {
        // Shipping OpenSBI reports a 4 MHz ACLINT timer; DTB overrides this.
        Ok(4_000_000)
    }
}

pub use VisionFive2Time as PlatformTimeImpl;
