//! OpenSBI HSM/IPI/remote-fence 运输层，供 RISC-V 机器 profile 共享。

use api_v0::smp::{HartStatus, PlatformSmp, PlatformSmpError, PlatformSmpResult};
use base::cpu::{CpuId, CpuMask};
use config::task::MAX_CPUS;

pub struct OpenSbiSmp;

fn result(ret : sbi::SbiRet) -> PlatformSmpResult<usize> {
    if ret.error == 0 {
        Ok(ret.value)
    } else {
        match ret.error as isize {
            -2 => Err(PlatformSmpError::Unsupported),
            -3 => Err(PlatformSmpError::InvalidCpu),
            -6 => Err(PlatformSmpError::AlreadyAvailable),
            _ => Err(PlatformSmpError::Firmware(ret.error)),
        }
    }
}

impl PlatformSmp for OpenSbiSmp {
    fn start_cpu(cpu : CpuId, start_addr : usize, opaque : usize) -> PlatformSmpResult<()> {
        if !cpu.fits_capacity(MAX_CPUS) || start_addr == 0 {
            return Err(PlatformSmpError::InvalidCpu);
        }
        result(sbi::hart_start(cpu.raw(), start_addr, opaque)).map(|_| ())
    }

    fn cpu_status(cpu : CpuId) -> PlatformSmpResult<HartStatus> {
        if !cpu.fits_capacity(MAX_CPUS) {
            return Err(PlatformSmpError::InvalidCpu);
        }
        Ok(match result(sbi::hart_get_status(cpu.raw()))? {
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
        result(sbi::send_ipi(sbi::HartMask::from_mask_base(mask.bits() as usize, 0))).map(|_| ())
    }

    fn flush_tlb_remote(mask : CpuMask) -> PlatformSmpResult<()> {
        result(sbi::remote_sfence_vma(sbi::HartMask::from_mask_base(mask.bits() as usize, 0),
                                      0,
                                      usize::MAX)).map(|_| ())
    }

    fn flush_icache_remote(mask : CpuMask) -> PlatformSmpResult<()> {
        result(sbi::remote_fence_i(sbi::HartMask::from_mask_base(mask.bits() as usize, 0)))
            .map(|_| ())
    }

    fn init_ipi() -> PlatformSmpResult<()> {
        Ok(())
    }
}

pub use OpenSbiSmp as SmpImpl;
