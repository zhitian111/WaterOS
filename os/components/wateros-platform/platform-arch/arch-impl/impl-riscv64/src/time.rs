use api_v0::time::{
    ArchTime, ArchTimeError, ArchTimeFrequency, ArchTimeResult, ArchTimeTick,
};
use core::arch::asm;

/// RISC-V 架构时间原语实现：
/// - 读取 CSR `time` 作为单调 tick
/// - 频率留给平台层（DTB/固件）做更准确的定义与组合
pub struct Riscv64ArchTime;

impl ArchTime for Riscv64ArchTime {
    #[inline]
    fn read_time_tick() -> ArchTimeResult<ArchTimeTick> {
        let v: u64;
        unsafe {
            asm!("csrr {0}, time", out(reg) v);
        }
        Ok(ArchTimeTick(v))
    }

    #[inline]
    fn read_time_frequency() -> ArchTimeResult<ArchTimeFrequency> {
        // 频率不在纯架构层硬编码：交由 platform 层根据硬件/DTB/固件定义。
        Err(ArchTimeError::Unsupported)
    }
}

