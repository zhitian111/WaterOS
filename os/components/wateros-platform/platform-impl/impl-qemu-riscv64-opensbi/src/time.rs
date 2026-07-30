//! QEMU RISC-V `virt` 的 timebase 频率回退值。

use api_v0::time::{PlatformTime, PlatformTimeError, PlatformTimeResult};

/// 尚未完成 DTB 探测时使用的 QEMU `virt` timebase 回退源。
pub struct QEMURiscv64OpenSBITime;

impl PlatformTime for QEMURiscv64OpenSBITime {
    #[inline]
    /// 返回 QEMU `virt` 的默认 10 MHz timebase；聚合层可用 DTB 值覆盖。
    fn get_time_frequency_hz() -> PlatformTimeResult<u64> {
        const QEMU_TIMEBASE_HZ: u64 = 10_000_000;
        if QEMU_TIMEBASE_HZ == 0 {
            Err(PlatformTimeError::InvalidFrequency)
        } else {
            Ok(QEMU_TIMEBASE_HZ)
        }
    }
}

pub use QEMURiscv64OpenSBITime as PlatformTimeImpl;
