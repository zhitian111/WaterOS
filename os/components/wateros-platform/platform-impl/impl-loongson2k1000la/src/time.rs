//! Loongson stable counter 频率：用 `cpucfg` 4/5 动态推导，失败时回退 100 MHz。

use api_v0::time::{PlatformTime, PlatformTimeError, PlatformTimeResult};

/// 未完成 CPUCFG 推导时的保守回退（板级 BSP 常见值，非硬件证据）。
pub const FALLBACK_TIMEBASE_HZ : u64 = 100_000_000;

#[cfg(target_arch = "loongarch64")]
fn cpucfg(index : usize) -> u32 {
    let mut value : usize;
    unsafe {
        core::arch::asm!("cpucfg {0}, {1}", out(reg) value, in(reg) index);
    }
    value as u32
}

#[cfg(not(target_arch = "loongarch64"))]
fn cpucfg(_index : usize) -> u32 {
    0
}

/// 用 CPUCFG 推导稳定计数器频率：`base * mul / div`（index 4/5）。
pub fn cpu_detected_timebase_hz() -> Option<u64> {
    let base = cpucfg(4) as u64;
    let cfg5 = cpucfg(5);
    let mul = (cfg5 & 0xFFFF) as u64;
    let div = (cfg5 >> 16) as u64;
    if base == 0 || div == 0 {
        return None;
    }
    base.checked_mul(mul)
        .map(|value| value / div)
}

pub struct Loongson2K1000Time;

impl PlatformTime for Loongson2K1000Time {
    fn get_time_frequency_hz() -> PlatformTimeResult<u64> {
        match cpu_detected_timebase_hz() {
            Some(hz) if hz != 0 => Ok(hz),
            _ => {
                if FALLBACK_TIMEBASE_HZ == 0 {
                    Err(PlatformTimeError::InvalidFrequency)
                } else {
                    Ok(FALLBACK_TIMEBASE_HZ)
                }
            }
        }
    }
}

pub use Loongson2K1000Time as PlatformTimeImpl;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_frequency_is_stable_and_documented() {
        assert_eq!(FALLBACK_TIMEBASE_HZ, 100_000_000);
        // 非 LoongArch 目标（host 测试）cpucfg 返回 0 → 走回退。
        assert_eq!(cpu_detected_timebase_hz(), None);
        assert_eq!(Loongson2K1000Time::get_time_frequency_hz(),
                   Ok(FALLBACK_TIMEBASE_HZ));
    }
}
