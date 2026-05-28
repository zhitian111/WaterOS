//! Sv39 下 **`satp`** 读写与 **`sfence.vma`** 冲刷的最小原语。
//!
//! 页表内容构造与用户/内核映射策略在 `wateros-mm`；此处仅保证切换根页表时 TLB
//! 一致性。

use core::arch::asm;

/// RISC-V Sv39 分页控制原语。
pub struct Riscv64Paging;

impl Riscv64Paging {
    /// 读取当前地址空间 token；在 RISC-V Sv39 下即 `satp` 当前值。
    #[inline]
    pub fn active_address_space_token() -> usize {
        let value : usize;
        unsafe {
            asm!("csrr {0}, satp", out(reg) value);
        }
        value
    }

    /// 激活指定地址空间 token；在 RISC-V Sv39 下即写入 `satp` 并执行全局
    /// `sfence.vma`。
    #[inline]
    pub fn activate_address_space_token_and_flush(token : usize) {
        unsafe {
            asm!("csrw satp, {0}", in(reg) token);
            asm!("sfence.vma x0, x0");
        }
    }

    /// 刷新当前地址空间下的地址翻译缓存；在 RISC-V 下即全局 `sfence.vma`。
    #[inline]
    pub fn flush_address_space_translations() {
        unsafe {
            asm!("sfence.vma x0, x0");
        }
    }

    /// RISC-V 下 MMU 由 OpenSBI 在 M 态管理，S 态不直接操作 MMU 使能位；no-op。
    #[inline]
    pub fn init_paging_disable_mmu() {}

    /// RISC-V 下 `satp` 的 MODE 字段决定分页模式；no-op（与 LoongArch
    /// 接口保持对称）。
    #[inline]
    pub fn enable_paging() {}
}
