//! LoongArch64 **分页控制原语**：通过 CSR.PGDL 读写根页表物理页号，
//! 通过 `invtlb` 指令刷新 TLB。
//!
//! 页表内容构造与用户/内核映射策略在 `wateros-mm`；此处仅保证切换根页表时 TLB
//! 一致性。

use core::arch::asm;

/// LoongArch64 分页控制原语。
pub struct LoongArch64Paging;

impl LoongArch64Paging {
    /// PGDL 寄存器编号 = 0x19。
    const CSR_PGDL : usize = 0x19;

    #[inline]
    fn read_pgdl() -> usize {
        let value : usize;
        unsafe {
            asm!("csrrd {0}, {1}", out(reg) value, const Self::CSR_PGDL);
        }
        value
    }

    #[inline]
    fn write_pgdl(token : usize) {
        unsafe {
            asm!("csrwr {0}, {1}", in(reg) token, const Self::CSR_PGDL);
        }
    }

    /// 全局 TLB 刷新。
    #[inline]
    fn invtlb_all() {
        unsafe {
            asm!("invtlb 0, $zero, $zero");
        }
    }

    #[inline]
    pub fn active_address_space_token() -> usize { Self::read_pgdl() }

    #[inline]
    pub fn activate_address_space_token_and_flush(token : usize) {
        Self::write_pgdl(token);
        Self::invtlb_all();
    }

    #[inline]
    pub fn flush_address_space_translations() { Self::invtlb_all(); }
}
