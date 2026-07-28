#![no_std]

//! QEMU **`virt` LoongArch64** 板级 profile：引导参数槽位、early console 与
//! StableCounter 频率常量。
//!
//! 与 `arch-impl-loongarch64` 组合使用；不包含 trap 帧或页表格式实现。

use core::arch::global_asm;

// 平台 shim 只解释固件参数；栈与普通启动流程由 arch boot 汇编负责。
global_asm!(include_str!("asm/_start.S"));

pub mod console;
/// ACPI GED 复位与关机后端。
pub mod reset;
/// Constant Timer deadline 编程后端。
pub mod timer;
pub mod smp {
    use api_v0::smp::{HartStatus, PlatformSmp, PlatformSmpError, PlatformSmpResult};
    use base::cpu::{CpuId, CpuMask};
    use core::sync::atomic::{AtomicU8, Ordering};

    const IOCSR_IPI_STATUS : usize = 0x1000;
    const IOCSR_IPI_EN : usize = 0x1004;
    const IOCSR_IPI_CLEAR : usize = 0x100c;
    const IOCSR_IPI_SEND : usize = 0x1040;
    const IOCSR_MBUF_SEND : usize = 0x1048;
    const IOCSR_IPI_SEND_BLOCKING : usize = 1 << 31;
    const IOCSR_IPI_SEND_CPU_SHIFT : usize = 16;
    const IOCSR_MBUF_SEND_BLOCKING : u64 = 1 << 31;
    const IOCSR_MBUF_SEND_BOX_SHIFT : usize = 2;
    const IOCSR_MBUF_SEND_CPU_SHIFT : usize = 16;
    const IOCSR_MBUF_SEND_BUF_SHIFT : usize = 32;
    const IOCSR_MBUF_SEND_H32_MASK : u64 = 0xffff_ffff_0000_0000;
    const IPI_BOOT_CPU : usize = 1 << 0;
    const IPI_RUNTIME_NOTIFICATION : usize = 1 << 1;

    const CPU_STOPPED : u8 = 0;
    const CPU_START_PENDING : u8 = 1;
    const CPU_STARTED : u8 = 2;
    static CPU_STATES : [AtomicU8; config::task::MAX_CPUS] =
        [const { AtomicU8::new(CPU_STOPPED) }; config::task::MAX_CPUS];

    #[inline]
    fn iocsr_read32(address : usize) -> u32 {
        let value : u32;
        unsafe {
            core::arch::asm!("iocsrrd.w {value}, {address}",
                             value = out(reg) value,
                             address = in(reg) address,
                             options(nostack));
        }
        value
    }

    #[inline]
    fn iocsr_write32(value : u32, address : usize) {
        unsafe {
            core::arch::asm!("iocsrwr.w {value}, {address}",
                             value = in(reg) value,
                             address = in(reg) address,
                             options(nostack));
        }
    }

    #[inline]
    fn iocsr_write64(value : u64, address : usize) {
        unsafe {
            core::arch::asm!("iocsrwr.d {value}, {address}",
                             value = in(reg) value,
                             address = in(reg) address,
                             options(nostack));
        }
    }

    #[inline]
    fn current_cpu_raw() -> usize {
        let cpu : usize;
        unsafe {
            core::arch::asm!("csrrd {cpu}, 0x20",
                             cpu = out(reg) cpu,
                             options(nomem, nostack));
        }
        cpu
    }

    /// QEMU 的 LoongArch 辅助核固件在 mailbox 0 上等待入口地址。写入顺序与
    /// Linux `csr_mail_send()` 一致：先高 32 位，再低 32 位。
    fn send_mailbox0(cpu : CpuId, data : u64) {
        let target = (cpu.raw() as u64) << IOCSR_MBUF_SEND_CPU_SHIFT;
        let high_box = 1u64 << IOCSR_MBUF_SEND_BOX_SHIFT;
        let high = IOCSR_MBUF_SEND_BLOCKING |
                   high_box |
                   target |
                   (data & IOCSR_MBUF_SEND_H32_MASK);
        iocsr_write64(high, IOCSR_MBUF_SEND);

        let low = IOCSR_MBUF_SEND_BLOCKING |
                  target |
                  (data << IOCSR_MBUF_SEND_BUF_SHIFT);
        iocsr_write64(low, IOCSR_MBUF_SEND);
    }

    #[inline]
    fn send_ipi_action(cpu : CpuId, action : usize) {
        let value = IOCSR_IPI_SEND_BLOCKING |
                    (cpu.raw() << IOCSR_IPI_SEND_CPU_SHIFT) |
                    action;
        iocsr_write32(value as u32, IOCSR_IPI_SEND);
    }

    pub struct QemuLoongArchSmp;
    impl PlatformSmp for QemuLoongArchSmp {
        fn start_cpu(cpu : CpuId, start_addr : usize, _: usize) -> PlatformSmpResult<()> {
            if !cpu.fits_capacity(config::task::MAX_CPUS) || start_addr == 0 {
                return Err(PlatformSmpError::InvalidCpu);
            }
            match CPU_STATES[cpu.raw()].compare_exchange(CPU_STOPPED,
                                                         CPU_START_PENDING,
                                                         Ordering::AcqRel,
                                                         Ordering::Acquire)
            {
                Ok(_) => {}
                Err(CPU_START_PENDING) | Err(CPU_STARTED) => {
                    return Err(PlatformSmpError::AlreadyAvailable);
                }
                Err(other) => return Err(PlatformSmpError::Firmware(other as usize)),
            }

            send_mailbox0(cpu, start_addr as u64);
            send_ipi_action(cpu, IPI_BOOT_CPU);
            Ok(())
        }

        fn cpu_status(cpu : CpuId) -> PlatformSmpResult<HartStatus> {
            if !cpu.fits_capacity(config::task::MAX_CPUS) {
                return Err(PlatformSmpError::InvalidCpu);
            }
            Ok(match CPU_STATES[cpu.raw()].load(Ordering::Acquire) {
                CPU_STOPPED => HartStatus::Stopped,
                CPU_START_PENDING => HartStatus::StartPending,
                CPU_STARTED => HartStatus::Started,
                other => HartStatus::Unknown(other as usize),
            })
        }

        fn configured_cpu_mask() -> CpuMask { CpuMask::from_bits((1u64 << config::task::MAX_CPUS) - 1) }

        fn send_ipi(mask : CpuMask) -> PlatformSmpResult<()> {
            for cpu in 0..config::task::MAX_CPUS {
                if !mask.contains(CpuId::from_raw(cpu)) { continue; }
                send_ipi_action(CpuId::from_raw(cpu), IPI_RUNTIME_NOTIFICATION);
            }
            Ok(())
        }

        fn flush_tlb_remote(_: CpuMask) -> PlatformSmpResult<()> {
            Err(PlatformSmpError::Unsupported)
        }

        fn init_ipi() -> PlatformSmpResult<()> {
            iocsr_write32(u32::MAX, IOCSR_IPI_EN);
            let cpu = current_cpu_raw();
            if cpu < config::task::MAX_CPUS {
                CPU_STATES[cpu].store(CPU_STARTED, Ordering::Release);
            }
            Ok(())
        }

        fn clear_ipi() -> PlatformSmpResult<()> {
            let pending = iocsr_read32(IOCSR_IPI_STATUS);
            if pending != 0 { iocsr_write32(pending, IOCSR_IPI_CLEAR); }
            Ok(())
        }
    }
    pub use QemuLoongArchSmp as SmpImpl;
}

pub mod boot {
    use api_v0::boot::{PlatformBootArgs, PlatformBootContext};

    /// 固件传入的原始 `a0`/`a1`/`a2`（具体含义随 QEMU/固件版本以调用约定为准）。
    // 本结构代码由AI完成
    #[derive(Debug, Clone, Copy)]
    pub struct QEMULoongArch64VirtBootArgs {
        arg0 : usize,
        arg1 : usize,
        arg2 : usize,
    }

    impl QEMULoongArch64VirtBootArgs {
        #[inline]
        pub const fn new(arg0 : usize, arg1 : usize, arg2 : usize) -> Self {
            Self { arg0, arg1, arg2 }
        }
    }

    impl PlatformBootArgs for QEMULoongArch64VirtBootArgs {
        #[inline]
        fn arg0(&self) -> Option<usize> { Some(self.arg0) }

        #[inline]
        fn arg1(&self) -> Option<usize> { Some(self.arg1) }

        #[inline]
        fn arg2(&self) -> Option<usize> { Some(self.arg2) }
    }

    /// 与 [`QEMULoongArch64VirtBootArgs`] 一一对应的类型化视图（当前为透传三槽）。
    // 本结构代码由AI完成
    #[derive(Debug, Clone, Copy)]
    pub struct QEMULoongArch64VirtBootContext {
        /// 固件 `a0`。
        pub arg0 : usize,
        /// 固件 `a1`。
        pub arg1 : usize,
        /// 固件 `a2`。
        pub arg2 : usize,
    }

    impl From<QEMULoongArch64VirtBootArgs> for QEMULoongArch64VirtBootContext {
        #[inline]
        fn from(value : QEMULoongArch64VirtBootArgs) -> Self {
            Self { arg0 : value.arg0,
                   arg1 : value.arg1,
                   arg2 : value.arg2 }
        }
    }

    impl PlatformBootContext<QEMULoongArch64VirtBootArgs> for QEMULoongArch64VirtBootContext {}

    pub use QEMULoongArch64VirtBootArgs as BootArgs;
    pub use QEMULoongArch64VirtBootContext as BootContext;
}

pub mod time {
    use api_v0::time::{PlatformTime, PlatformTimeError, PlatformTimeResult};

    /// QEMU virt 上 StableCounter 常用频率（Hz）；与 arch `rdtime.d` 刻度一致。
    // 本结构代码由AI完成
    pub struct QEMULoongArch64VirtTime;

    impl PlatformTime for QEMULoongArch64VirtTime {
        #[inline]
        fn get_time_frequency_hz() -> PlatformTimeResult<u64> {
            // QEMU Constant Timer：`TIMER_PERIOD = 10` ns → 100 MHz（见 QEMU `cpucfg.c`）。
            // DTB virt 通常无 CPU timebase 属性；引导期探测未命中时回退此值。
            const QEMU_LOONGARCH64_TIMEBASE_HZ : u64 = 100_000_000;
            if QEMU_LOONGARCH64_TIMEBASE_HZ == 0 {
                Err(PlatformTimeError::InvalidFrequency)
            } else {
                Ok(QEMU_LOONGARCH64_TIMEBASE_HZ)
            }
        }
    }

    pub use QEMULoongArch64VirtTime as PlatformTimeImpl;
}
