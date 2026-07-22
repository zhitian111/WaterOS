//! 当前正在执行本段内核代码的逻辑 CPU（LoongArch 占位实现）。

use base::cpu::CpuId;
use api_v0::cpu::{ArchCpuInitError, ArchCpuInitResult};
use config::task::MAX_CPUS;

/// 返回当前正在执行本段内核代码的逻辑 CPU。
/// LoongArch 单核 bring-up 阶段暂时返回 CPU 0。
pub fn current_cpu_id() -> CpuId {
    let cpu_id : usize;
    unsafe {
        core::arch::asm!("csrrd {}, 0x10", out(reg) cpu_id);
    }
    CpuId::from_raw(cpu_id)
}

pub fn init_current_cpu(cpu : CpuId) -> ArchCpuInitResult<()> {
    if !cpu.fits_capacity(MAX_CPUS) || current_cpu_id() != cpu {
        return Err(ArchCpuInitError::InvalidCpu);
    }
    Ok(())
}
