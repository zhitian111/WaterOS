//! 不支持 SMP 的占位后端。

use api_v0::smp::{HartStatus, PlatformSmp, PlatformSmpError, PlatformSmpResult};
use base::cpu::{CpuId, CpuMask};

pub struct DummySmp;

impl PlatformSmp for DummySmp {
    fn start_cpu(_ : CpuId, _ : usize, _ : usize) -> PlatformSmpResult<()> {
        Err(PlatformSmpError::Unsupported)
    }
    fn cpu_status(_ : CpuId) -> PlatformSmpResult<HartStatus> { Err(PlatformSmpError::Unsupported) }
    fn configured_cpu_mask() -> CpuMask { CpuMask::EMPTY }
    fn send_ipi(_ : CpuMask) -> PlatformSmpResult<()> { Err(PlatformSmpError::Unsupported) }
    fn flush_tlb_remote(_ : CpuMask) -> PlatformSmpResult<()> { Err(PlatformSmpError::Unsupported) }
    fn init_ipi() -> PlatformSmpResult<()> { Err(PlatformSmpError::Unsupported) }
}

pub use DummySmp as SmpImpl;
