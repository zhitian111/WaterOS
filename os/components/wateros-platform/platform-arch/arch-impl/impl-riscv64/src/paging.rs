//! Sv39 下 **`satp`** 读写与 **`sfence.vma`** 冲刷的最小原语。
//!
//! 页表内容构造与用户/内核映射策略在 `wateros-mm`；此处仅保证切换根页表时 TLB 一致性。

use core::arch::asm;

/// RISC-V Sv39 分页控制原语。
pub struct Riscv64Paging;

impl Riscv64Paging {
    /// 读取 satp 当前值，用于分页启用前后的观测。
    #[inline]
    pub fn read_satp() -> usize {
        let value: usize;
        unsafe {
            asm!("csrr {0}, satp", out(reg) value);
        }
        value
    }

    /// 写入 satp 后立刻执行全局 sfence.vma，确保页表切换后翻译可见。
    #[inline]
    pub fn write_satp_and_flush(satp: usize) {
        unsafe {
            asm!("csrw satp, {0}", in(reg) satp);
            asm!("sfence.vma x0, x0");
        }
    }

    /// 在不切换 `satp` 的前提下，对当前根页表已修改的 PTE 做全局 TLB 一致性冲刷。
    #[inline]
    pub fn sfence_vma_all() {
        unsafe {
            asm!("sfence.vma x0, x0");
        }
    }
}
