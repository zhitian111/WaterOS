//! QEMU LoongArch `virt` StableCounter 频率回退值。

use api_v0::time::{PlatformTime, PlatformTimeError, PlatformTimeResult};

/// StableCounter 频率的启动期回退源；DTB/firmware 将来可在聚合层覆盖它。
pub struct QEMULoongArch64VirtTime;

impl PlatformTime for QEMULoongArch64VirtTime {
    #[inline]
    /// 返回 QEMU LoongArch `virt` 当前约定的 100 MHz StableCounter 频率。
    fn get_time_frequency_hz() -> PlatformTimeResult<u64> {
        const QEMU_LOONGARCH64_TIMEBASE_HZ: u64 = 100_000_000;
        if QEMU_LOONGARCH64_TIMEBASE_HZ == 0 {
            Err(PlatformTimeError::InvalidFrequency)
        } else {
            Ok(QEMU_LOONGARCH64_TIMEBASE_HZ)
        }
    }
}

pub use QEMULoongArch64VirtTime as PlatformTimeImpl;
