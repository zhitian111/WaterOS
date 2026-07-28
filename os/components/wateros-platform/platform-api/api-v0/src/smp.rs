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

/// Software-level reason carried alongside the hardware IPI notification.
/// SBI and platform IPI registers only deliver the interrupt signal itself.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpiKind {
    Reschedule = 1 << 0,
    TlbShootdown = 1 << 1,
    /// 目标任务有必须在其 trap-return 安全点处理的状态变化（如 signal）。
    TaskNotify = 1 << 2,
}

impl IpiKind {
    #[inline]
    pub const fn bits(self) -> u8 { self as u8 }
}

pub trait PlatformSmp {
    fn start_cpu(cpu : CpuId, start_addr: usize, opaque: usize) -> PlatformSmpResult<()>;
    fn cpu_status(cpu : CpuId) -> PlatformSmpResult<HartStatus>;
    fn configured_cpu_mask() -> CpuMask;
    fn send_ipi(mask : CpuMask) -> PlatformSmpResult<()>;
    /// Synchronously invalidate all address translations on the selected CPUs.
    ///
    /// Firmware-backed platforms may complete this without requiring the
    /// target CPUs to take a supervisor software interrupt.
    fn flush_tlb_remote(mask : CpuMask) -> PlatformSmpResult<()>;
    fn init_ipi() -> PlatformSmpResult<()>;
    fn clear_ipi() -> PlatformSmpResult<()>;
}
