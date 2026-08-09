use api_v0::time::{PlatformTime, PlatformTimeResult};
pub struct Loongson2K1000LATime;
impl PlatformTime for Loongson2K1000LATime {
    fn get_time_frequency_hz() -> PlatformTimeResult<u64> {
        // TODO(real-hardware): replace after boot-parameter/CPUCFG frequency validation.
        Ok(100_000_000)
    }
}
pub use Loongson2K1000LATime as PlatformTimeImpl;
