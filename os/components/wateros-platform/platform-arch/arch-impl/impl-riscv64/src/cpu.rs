//! 当前正在执行本段内核代码的逻辑 CPU。

use base::cpu::CpuId;

/// 返回当前正在执行本段内核代码的逻辑 CPU。
///
/// 实现：读 `tp` 寄存器——`_start.S` 入口处已将 OpenSBI 传入的 `a0` (hart_id)
/// 保存到 `tp`。S 态内核 trap 会保存/恢复 `tp` 寄存器，用户态 trap 中 `tp`
/// 被用户 TLS 指针覆盖；但在内核启动阶段（进入用户态前）`tp` 始终有效。
/// 不可读 `mhartid`（M 态 CSR，S 态下触发 IllegalInstruction 异常）。
pub fn current_cpu_id() -> CpuId {
    let hart_id : usize;
    // SAFETY: 内核启动阶段 _start.S 写入 tp = hart_id，之后在用户态 trap 上下
    // 文外均保持不变。从用户态 trap 进入内核时 tp 为用户 TLS，但那时不应调用
    // current_cpu_id()——应改用 per-CPU 数据结构。
    unsafe {
        core::arch::asm!("mv {}, tp", out(reg) hart_id);
    }
    CpuId::from_raw(hart_id)
}
