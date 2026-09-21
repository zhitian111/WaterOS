//! 当前正在执行本段内核代码的逻辑 CPU。

use api_v0::cpu::{ArchCpuInitError, ArchCpuInitResult};
use base::cpu::CpuId;
use config::task::MAX_CPUS;

/// 返回当前正在执行本段内核代码的逻辑 CPU。
///
/// QEMU virt 的 `CSR.CPUID` 位于 CSR `0x20`。该值必须与 boot mailbox 的 CPU 编号
/// 一致，否则 AP 在 `init_current_cpu` 阶段被拒绝，不能继续进入 scheduler。
pub fn current_cpu_id() -> CpuId {
    let cpu_id: usize;
    unsafe {
        core::arch::asm!("csrrd {}, 0x20", out(reg) cpu_id);
    }
    CpuId::from_raw(cpu_id & 0x1ff)
}

/// 验证启动代码传入的逻辑 CPU 与本 hart 的硬件 CPUID 一致。
///
/// BOOT_CONTRACT: LoongArch 当前没有像 RISC-V `tp` 那样的额外设置；但必须在
/// `MAX_CPUS` 范围内校验，以免静态 per-CPU 数组越界。
pub fn init_current_cpu(cpu: CpuId) -> ArchCpuInitResult<()> {
    if !cpu.fits_capacity(MAX_CPUS) || current_cpu_id() != cpu {
        return Err(ArchCpuInitError::InvalidCpu);
    }
    Ok(())
}
