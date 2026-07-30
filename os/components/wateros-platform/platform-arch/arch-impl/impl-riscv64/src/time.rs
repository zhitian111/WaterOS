//! 通过 CSR **`time`** 提供单调 tick；**时钟频率**不在此硬编码，由上层根据 DTB/固件填充或返回 `ArchTimeError::Unsupported`。
//!
//! 与 trap 路径中定时器中断重新武装配合时，应使用同一 tick 语义。

use api_v0::time::{ArchTime, ArchTimeError, ArchTimeFrequency, ArchTimeResult, ArchTimeTick};
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
