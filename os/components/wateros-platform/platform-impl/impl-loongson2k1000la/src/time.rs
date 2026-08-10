use api_v0::time::{PlatformTime, PlatformTimeError, PlatformTimeResult};

/// Current BSP fallback for the Loongson stable counter.
///
/// `os::boot_timebase` should replace this during boot when firmware supplies
/// `/cpus/timebase-frequency`. Until a board confirms the counter source, this
/// value remains explicitly a fallback and is not hardware evidence.
pub const FALLBACK_TIMEBASE_HZ: u64 = 100_000_000;

pub const fn validate_frequency_hz(hz: u64) -> PlatformTimeResult<u64> {
    if hz == 0 {
        Err(PlatformTimeError::InvalidFrequency)
    } else {
        Ok(hz)
    }
}

pub struct Loongson2K1000LATime;
impl PlatformTime for Loongson2K1000LATime {
    fn get_time_frequency_hz() -> PlatformTimeResult<u64> {
        validate_frequency_hz(FALLBACK_TIMEBASE_HZ)
    }
}
pub use Loongson2K1000LATime as PlatformTimeImpl;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_frequency_is_stable_and_documented() {
        assert_eq!(Loongson2K1000LATime::get_time_frequency_hz(),
                   Ok(FALLBACK_TIMEBASE_HZ));
        assert_eq!(validate_frequency_hz(0), Err(PlatformTimeError::InvalidFrequency));
        assert_eq!(validate_frequency_hz(1), Ok(1));
    }
}
