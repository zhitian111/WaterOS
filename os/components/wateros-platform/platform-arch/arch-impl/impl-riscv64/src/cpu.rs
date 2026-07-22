//! 当前正在执行本段内核代码的逻辑 CPU。

use base::cpu::CpuId;
use api_v0::cpu::{ArchCpuInitError, ArchCpuInitResult};
use config::task::MAX_CPUS;

const RETURN_FRAME_BYTES : usize = 40 * core::mem::size_of::<usize>();
const RETURN_FRAME_CPU_ID_OFFSET : usize = 38 * core::mem::size_of::<usize>();

unsafe extern "C" {
    static mut __wateros_riscv_return_frames: u8;
}

/// 返回当前正在执行本段内核代码的逻辑 CPU。
///
/// 实现：读内核 `tp`。用户 trap 汇编在进入任何 Rust 代码前从当前 CPU 的
/// supervisor-only return frame 恢复该值。
/// 不可读 `mhartid`（M 态 CSR，S 态下触发 IllegalInstruction 异常）。
pub fn current_cpu_id() -> CpuId {
    let hart_id : usize;
    // SAFETY: _start.S 和用户 trap 入口保证进入 Rust 时 tp 为逻辑 CPU id。
    unsafe {
        core::arch::asm!("mv {}, tp", out(reg) hart_id, options(nomem, nostack));
    }
    CpuId::from_raw(hart_id)
}

/// 初始化本 CPU 的可信 `tp` 和 trampoline return-frame CPU id 槽。
pub fn init_current_cpu(cpu : CpuId) -> ArchCpuInitResult<()> {
    if !cpu.fits_capacity(MAX_CPUS) {
        return Err(ArchCpuInitError::InvalidCpu);
    }
    unsafe {
        core::arch::asm!("mv tp, {}", in(reg) cpu.raw(), options(nomem, nostack));
        let slot = core::ptr::addr_of_mut!(__wateros_riscv_return_frames)
                       .add(cpu.index() * RETURN_FRAME_BYTES + RETURN_FRAME_CPU_ID_OFFSET)
                       .cast::<usize>();
        core::ptr::write_volatile(slot, cpu.raw());
    }
    Ok(())
}
