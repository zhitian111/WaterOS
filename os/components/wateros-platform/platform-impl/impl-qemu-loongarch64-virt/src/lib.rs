#![no_std]

//! QEMU **`virt` LoongArch64** 板级 profile：引导参数槽位、early console 与
//! StableCounter 频率常量。
//!
//! 与 `arch-impl-loongarch64` 组合使用；不包含 trap 帧或页表格式实现。

pub mod console;
pub mod reset;
pub mod timer;

pub mod boot {
    use api_v0::boot::{PlatformBootArgs, PlatformBootContext};

    /// 固件传入的原始 `a0`/`a1`/`a2`（具体含义随 QEMU/固件版本以调用约定为准）。
    #[derive(Debug, Clone, Copy)]
    pub struct QEMULoongArch64VirtBootArgs {
        arg0: usize,
        arg1: usize,
        arg2: usize,
    }

    impl QEMULoongArch64VirtBootArgs {
        #[inline]
        pub const fn new(arg0: usize, arg1: usize, arg2: usize) -> Self {
            Self { arg0, arg1, arg2 }
        }
    }

    impl PlatformBootArgs for QEMULoongArch64VirtBootArgs {
        #[inline]
        fn arg0(&self) -> Option<usize> {
            Some(self.arg0)
        }

        #[inline]
        fn arg1(&self) -> Option<usize> {
            Some(self.arg1)
        }

        #[inline]
        fn arg2(&self) -> Option<usize> {
            Some(self.arg2)
        }
    }

    /// 与 [`QEMULoongArch64VirtBootArgs`] 一一对应的类型化视图（当前为透传三槽）。
    #[derive(Debug, Clone, Copy)]
    pub struct QEMULoongArch64VirtBootContext {
        pub arg0: usize,
        pub arg1: usize,
        pub arg2: usize,
    }

    impl From<QEMULoongArch64VirtBootArgs> for QEMULoongArch64VirtBootContext {
        #[inline]
        fn from(value: QEMULoongArch64VirtBootArgs) -> Self {
            Self {
                arg0: value.arg0,
                arg1: value.arg1,
                arg2: value.arg2,
            }
        }
    }

    impl PlatformBootContext<QEMULoongArch64VirtBootArgs> for QEMULoongArch64VirtBootContext {}

    pub use QEMULoongArch64VirtBootArgs as BootArgs;
    pub use QEMULoongArch64VirtBootContext as BootContext;
}

pub mod time {
    use api_v0::time::{PlatformTime, PlatformTimeError, PlatformTimeResult};

    /// QEMU virt 上 StableCounter 常用频率（Hz）；与 arch `rdtime.d` 刻度一致。
    pub struct QEMULoongArch64VirtTime;

    impl PlatformTime for QEMULoongArch64VirtTime {
        #[inline]
        fn time_frequency_hz() -> PlatformTimeResult<u64> {
            // QEMU Constant Timer：`TIMER_PERIOD = 10` ns → 100 MHz（见 QEMU `cpucfg.c`）。
            // DTB virt 通常无 CPU timebase 属性；引导期探测未命中时回退此值。
            const QEMU_LOONGARCH64_TIMEBASE_HZ: u64 = 100_000_000;
            if QEMU_LOONGARCH64_TIMEBASE_HZ == 0 {
                Err(PlatformTimeError::InvalidFrequency)
            } else {
                Ok(QEMU_LOONGARCH64_TIMEBASE_HZ)
            }
        }
    }

    pub use QEMULoongArch64VirtTime as PlatformTimeImpl;
}
