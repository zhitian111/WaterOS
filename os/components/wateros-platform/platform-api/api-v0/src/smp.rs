//! Platform SMP lifecycle types.  CPU ids are logical ids selected by the
//! platform; the QEMU RISC-V profile currently uses the hart id directly.

use base::cpu::{CpuId, CpuMask};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformSmpError {
    Unsupported,
    InvalidCpu,
    /// The hart has already been started by firmware.  Callers should still
    /// wait for the OS-level online acknowledgement before using it.
    AlreadyAvailable,
    Firmware(usize),
}

pub type PlatformSmpResult<T> = core::result::Result<T, PlatformSmpError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HartStatus {
    Started,
    Stopped,
    StartPending,
    StopPending,
    Unknown(usize),
}

pub trait PlatformSmp {
    fn start_cpu(cpu : CpuId, start_addr: usize, opaque: usize) -> PlatformSmpResult<()>;
    fn cpu_status(cpu : CpuId) -> PlatformSmpResult<HartStatus>;
    fn configured_cpu_mask() -> CpuMask;
    fn send_ipi(mask : CpuMask) -> PlatformSmpResult<()>;
    fn init_ipi() -> PlatformSmpResult<()>;
    fn clear_ipi() -> PlatformSmpResult<()>;
}
