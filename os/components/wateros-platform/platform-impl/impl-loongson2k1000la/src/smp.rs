use api_v0::smp::{HartStatus, PlatformSmp, PlatformSmpError, PlatformSmpResult};
use base::cpu::{CpuId, CpuMask};

pub struct Loongson2K1000LASmp;

impl PlatformSmp for Loongson2K1000LASmp {
    fn start_cpu(_ : CpuId, _ : usize, _ : usize) -> PlatformSmpResult<()> {
        Err(PlatformSmpError::Unsupported)
    }
    fn cpu_status(cpu : CpuId) -> PlatformSmpResult<HartStatus> {
        if cpu.raw() == 0 {
            Ok(HartStatus::Started)
        } else {
            Err(PlatformSmpError::Unsupported)
        }
    }
    fn configured_cpu_mask() -> CpuMask { CpuMask::from_bits(1) }
    fn send_ipi(_ : CpuMask) -> PlatformSmpResult<()> { Err(PlatformSmpError::Unsupported) }
    fn flush_tlb_remote(_ : CpuMask) -> PlatformSmpResult<()> { Err(PlatformSmpError::Unsupported) }
    fn init_ipi() -> PlatformSmpResult<()> { Ok(()) }
}

/// Until the legacy boot-parameter CPU table is parsed, boot only the BSP.
pub fn init_configured_cpu_mask(_ : usize) -> Result<CpuMask, PlatformSmpError> {
    Ok(CpuMask::from_bits(1))
}

pub use Loongson2K1000LASmp as SmpImpl;
