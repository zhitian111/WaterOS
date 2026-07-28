//! 当前正在执行本段内核代码的逻辑 CPU。

use base::cpu::CpuId;
use api_v0::cpu::{ArchCpuInitError, ArchCpuInitResult};
use config::task::MAX_CPUS;

/// 返回当前正在执行本段内核代码的逻辑 CPU。
/// QEMU virt 的 `CSR.CPUID` 位于 CSR `0x20`。
pub fn current_cpu_id() -> CpuId {
    let cpu_id : usize;
    unsafe {
        core::arch::asm!("csrrd {}, 0x20", out(reg) cpu_id);
    }
    CpuId::from_raw(cpu_id)
}

pub fn init_current_cpu(cpu : CpuId) -> ArchCpuInitResult<()> {
    if !cpu.fits_capacity(MAX_CPUS) || current_cpu_id() != cpu {
        return Err(ArchCpuInitError::InvalidCpu);
    }
    Ok(())
}
