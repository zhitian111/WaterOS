#![no_std]

//! QEMU `virt` 机器上 **RISC-V + OpenSBI** 的板级约定：`a0`/`a1` 分别承载 hart id
//! 与 DTB 物理地址等常见调用约定，时间频率当前为常量（可后续改为读 DTB）。
//!
//! 本 crate 属于 **platform-impl**：描述运行环境假设，不包含 ISA 细节实现
//!（见 `wateros-platform-arch-impl-riscv64`），但会接线该平台 profile 使用的
//! OpenSBI console、timer 与 reset 后端。

use core::arch::global_asm;

// 平台 shim 只解释 OpenSBI 参数；栈与普通启动流程由 arch boot 汇编负责。
global_asm!(include_str!("asm/_start.S"));

pub mod console;
/// OpenSBI system reset 后端。
pub mod reset;
/// OpenSBI timer 后端（经 SBI 设置下次中断时刻）。
pub mod timer;
/// SBI HSM based secondary-hart control for QEMU RISC-V.
pub mod smp {
    use api_v0::smp::{HartStatus, PlatformSmp, PlatformSmpError, PlatformSmpResult};
    use base::cpu::{CpuId, CpuMask};
    use config::task::MAX_CPUS;

    pub struct QemuRiscv64OpenSbiSmp;

    fn result(ret: sbi::SbiRet) -> PlatformSmpResult<usize> {
        if ret.error == 0 {
            Ok(ret.value)
        } else {
            match ret.error as isize {
                // SBI_ERR_NOT_SUPPORTED: HSM is absent from this firmware.
                -2 => Err(PlatformSmpError::Unsupported),
                // SBI_ERR_INVALID_PARAM: QEMU has no such hart in this machine.
                -3 => Err(PlatformSmpError::InvalidCpu),
                // SBI_ERR_ALREADY_AVAILABLE: firmware had already started it.
                -6 => Err(PlatformSmpError::AlreadyAvailable),
                _ => Err(PlatformSmpError::Firmware(ret.error)),
            }
        }
    }

    impl PlatformSmp for QemuRiscv64OpenSbiSmp {
        fn start_cpu(cpu: CpuId, start_addr: usize, opaque: usize) -> PlatformSmpResult<()> {
            if !cpu.fits_capacity(MAX_CPUS) { return Err(PlatformSmpError::InvalidCpu); }
            result(sbi::hart_start(cpu.raw(), start_addr, opaque)).map(|_| ())
        }

        fn cpu_status(cpu: CpuId) -> PlatformSmpResult<HartStatus> {
            if !cpu.fits_capacity(MAX_CPUS) { return Err(PlatformSmpError::InvalidCpu); }
            let value = result(sbi::hart_get_status(cpu.raw()))?;
            Ok(match value {
                0 => HartStatus::Started,
                1 => HartStatus::Stopped,
                2 => HartStatus::StartPending,
                3 => HartStatus::StopPending,
                other => HartStatus::Unknown(other),
            })
        }

        fn configured_cpu_mask() -> CpuMask {
            CpuMask::from_bits((1u64 << MAX_CPUS) - 1)
        }

        fn send_ipi(mask : CpuMask) -> PlatformSmpResult<()> {
            let hart_mask = sbi::HartMask::from_mask_base(mask.bits() as usize, 0);
            result(sbi::send_ipi(hart_mask)).map(|_| ())
        }

        fn flush_tlb_remote(mask : CpuMask) -> PlatformSmpResult<()> {
            let hart_mask = sbi::HartMask::from_mask_base(mask.bits() as usize, 0);
            result(sbi::remote_sfence_vma(hart_mask, 0, usize::MAX)).map(|_| ())
        }

        fn init_ipi() -> PlatformSmpResult<()> { Ok(()) }

        /// OpenSBI `send_ipi` raises supervisor software interrupt (SSIP) on
        /// the target hart.  The receiver must clear its local `sip.SSIP`
        /// bit before returning from the trap; otherwise the hart immediately
        /// re-enters the same interrupt forever.
        fn clear_ipi() -> PlatformSmpResult<()> {
            unsafe {
                core::arch::asm!("csrc sip, {}",
                                 in(reg) 1usize << 1,
                                 options(nomem, nostack));
            }
            Ok(())
        }
    }

    pub use QemuRiscv64OpenSbiSmp as SmpImpl;
}

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
