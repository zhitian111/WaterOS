//! RISC-V 监督态核间中断（IPI）：通过 SBI `send_ipi` 向目标 hart 发送 Supervisor Soft Interrupt。

use base::cpu::CpuMask;
use sbi::{HartMask, SbiRet};

/// 架构无关 IPI 门面返回的失败原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpiError {
    /// SBI/固件返回的原始错误码。
    Firmware(usize),
    /// 当前 profile 未提供 IPI 能力。
    Unsupported,
}

/// 将 `SbiRet` 转换为 `Result<(), ()>`：`error == 0` 为成功。
#[inline]
fn sbi_ok(ret: SbiRet) -> Result<(), IpiError> {
    if ret.error == 0 {
        Ok(())
    } else {
        Err(IpiError::Firmware(ret.error))
    }
}

/// 向 `cpu_mask` 指定的所有 hart 发送核间软中断。
///
/// 接收方将通过 `SupervisiorSoft` trap 进入内核，处理函数应调用
/// [`super::interrupt::clear_soft_interrupt`] 清除 SSIP 位。
#[inline]
pub fn send_ipi(cpu_mask: CpuMask) -> Result<(), IpiError> {
    let hart_mask = HartMask::from_mask_base(cpu_mask.bits() as usize, 0);
    sbi_ok(sbi::send_ipi(hart_mask))
}
