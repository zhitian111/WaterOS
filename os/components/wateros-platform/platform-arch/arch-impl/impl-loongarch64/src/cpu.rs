//! 当前正在执行本段内核代码的逻辑 CPU（LoongArch 占位实现）。

use base::cpu::CpuId;

/// 返回当前正在执行本段内核代码的逻辑 CPU。
/// LoongArch 单核 bring-up 阶段暂时返回 CPU 0。
pub fn current_cpu_id() -> CpuId {
    let cpu_id : usize;
    unsafe {
        core::arch::asm!("csrrd {}, 0x10", out(reg) cpu_id);
    }
    CpuId::from_raw(cpu_id)
}
