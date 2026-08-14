//! SMP 占位：任务 09 与真实固件协议对齐后再启用。

use api_v0::smp::{HartStatus, PlatformSmp, PlatformSmpError, PlatformSmpResult};
use base::cpu::{CpuId, CpuMask};

pub struct Loongson2K1000Smp;

impl PlatformSmp for Loongson2K1000Smp {
    fn start_cpu(_cpu : CpuId, _start_addr : usize, _opaque : usize) -> PlatformSmpResult<()> {
        Err(PlatformSmpError::Unsupported)
    }

    fn cpu_status(_cpu : CpuId) -> PlatformSmpResult<HartStatus> {
        Err(PlatformSmpError::Unsupported)
    }

    fn configured_cpu_mask() -> CpuMask {
        CpuMask::EMPTY
    }

    fn send_ipi(_mask : CpuMask) -> PlatformSmpResult<()> {
        Err(PlatformSmpError::Unsupported)
    }

    fn flush_tlb_remote(_mask : CpuMask) -> PlatformSmpResult<()> {
        Err(PlatformSmpError::Unsupported)
    }

    fn flush_icache_remote(_mask : CpuMask) -> PlatformSmpResult<()> {
        Err(PlatformSmpError::Unsupported)
    }

    fn init_ipi() -> PlatformSmpResult<()> {
        Err(PlatformSmpError::Unsupported)
    }
}

pub use Loongson2K1000Smp as SmpImpl;
