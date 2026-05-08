//! LoongArch64 **StableCounter**：`rdtime.d` 读单调 tick；频率由 platform 层提供（本实现
//! 对 `read_time_frequency` 返回不支持）。

use api_v0::time::{ArchTime, ArchTimeError, ArchTimeFrequency, ArchTimeResult, ArchTimeTick};
use core::arch::asm;

/// LoongArch64 StableCounter 时间原语实现。
pub struct LoongArch64ArchTime;

impl ArchTime for LoongArch64ArchTime {
    #[inline]
    fn read_time_tick() -> ArchTimeResult<ArchTimeTick> {
        let tick: u64;
        let _counter_id: usize;
        unsafe {
            asm!("rdtime.d {0}, {1}", out(reg) tick, out(reg) _counter_id);
        }
        Ok(ArchTimeTick(tick))
    }

    #[inline]
    fn read_time_frequency() -> ArchTimeResult<ArchTimeFrequency> {
        Err(ArchTimeError::Unsupported)
    }
}
