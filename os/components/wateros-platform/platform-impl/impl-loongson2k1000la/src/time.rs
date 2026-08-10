use api_v0::time::{PlatformTime, PlatformTimeResult};
pub struct Loongson2K1000LATime;
impl PlatformTime for Loongson2K1000LATime {
    fn get_time_frequency_hz() -> PlatformTimeResult<u64> {
        // TODO(real-hardware): replace after boot-parameter/CPUCFG frequency validation.
        Ok(100_000_000)
    }
}
pub use Loongson2K1000LATime as PlatformTimeImpl;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_frequency_is_stable_and_documented() {
        // This is the current BSP placeholder, not hardware calibration evidence.
        assert_eq!(Loongson2K1000LATime::get_time_frequency_hz(), Ok(100_000_000));
    }
}
