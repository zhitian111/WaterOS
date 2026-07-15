#![no_std]

//! QEMU `virt` 机器上 **RISC-V + OpenSBI** 的板级约定：`a0`/`a1` 分别承载 hart id
//! 与 DTB 物理地址等常见调用约定，时间频率当前为常量（可后续改为读 DTB）。
//!
//! 本 crate 属于 **platform-impl**：描述运行环境假设，不包含 ISA 细节实现
//!（见 `wateros-platform-arch-impl-riscv64`），但会接线该平台 profile 使用的
//! OpenSBI console、timer 与 reset 后端。

pub mod console;
/// OpenSBI system reset 后端。
pub mod reset;
/// OpenSBI timer 后端（经 SBI 设置下次中断时刻）。
pub mod timer;

pub mod boot {
    use api_v0::boot::{PlatformBootArgs, PlatformBootContext};
    /// OpenSBI 传入的原始参数槽位（`a0`/`a1` 等由 [`PlatformBootArgs`] 方法暴露）。
    // 本结构代码由AI完成
    #[derive(Debug, Clone, Copy)]
    #[allow(unused)]
    pub struct QEMURiscv64OpenSBIBootArgs {
        /// 固件 `a0`（通常为 hart id）。
        _arg0 : usize,
        /// 固件 `a1`（通常为 DTB 物理地址）。
        _arg1 : usize,
    }
    impl PlatformBootArgs for QEMURiscv64OpenSBIBootArgs {
        #[inline]
        fn arg0(&self) -> Option<usize> { Some(self._arg0) }
        #[inline]
        fn arg1(&self) -> Option<usize> { Some(self._arg1) }
    }
    impl QEMURiscv64OpenSBIBootArgs {
        #[inline]
        #[allow(unused)]
        pub fn new(arg0 : usize, arg1 : usize) -> Self {
            Self { _arg0 : arg0,
                   _arg1 : arg1 }
        }
    }
    /// 类型化引导上下文：hart id 与 DTB 物理地址。
    // 本结构代码由AI完成
    #[derive(Debug, Clone, Copy)]
    #[allow(unused)]
    pub struct QEMURiscv64OpenSBIBootContext {
        /// 启动 hart id。
        _hart_id : base::cpu::CPUHartID,
        /// 设备树 blob 物理地址。
        _dtb_pa : base::boot::DTBPA,
    }
    impl From<QEMURiscv64OpenSBIBootArgs> for QEMURiscv64OpenSBIBootContext {
        #[inline]
        #[allow(unused)]
        fn from(value : QEMURiscv64OpenSBIBootArgs) -> Self {
            let hart_id = value.arg0()
                               .expect("OpenSBIBoot args error in arg0");
            let dtb_pa = value.arg1()
                              .expect("OpenSBIBoot args error in arg1");
            let dtb_pa = base::addr::BasePhysAddr { val : dtb_pa };
            Self { _hart_id : hart_id,
                   _dtb_pa : dtb_pa }
        }
    }
    impl PlatformBootContext<QEMURiscv64OpenSBIBootArgs> for QEMURiscv64OpenSBIBootContext {}

    pub use QEMURiscv64OpenSBIBootArgs as BootArgs;
    pub use QEMURiscv64OpenSBIBootContext as BootContext;
}

pub mod time {
    use api_v0::time::{PlatformTime, PlatformTimeError, PlatformTimeResult};

    /// QEMU virt 上常用的 timebase 频率（Hz）；引导期可由 DTB 覆盖，此处为回退常量。
    // 本结构代码由AI完成
    pub struct QEMURiscv64OpenSBITime;

    impl PlatformTime for QEMURiscv64OpenSBITime {
        #[inline]
        fn get_time_frequency_hz() -> PlatformTimeResult<u64> {
            // QEMU virt DTB `/cpus/timebase-frequency` 默认 10 MHz；未探测时的回退。
            const QEMU_TIMEBASE_HZ : u64 = 10_000_000;
            if QEMU_TIMEBASE_HZ == 0 {
                Err(PlatformTimeError::InvalidFrequency)
            } else {
                Ok(QEMU_TIMEBASE_HZ)
            }
        }
    }

    pub use QEMURiscv64OpenSBITime as PlatformTimeImpl;
}
