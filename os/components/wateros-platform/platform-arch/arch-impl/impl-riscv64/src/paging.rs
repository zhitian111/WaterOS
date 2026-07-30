//! Sv39 下 **`satp`** 读写与 **`sfence.vma`** 冲刷的最小分页控制原语。
//!
//! 页表内容构造与用户/内核映射策略在 `wateros-mm`；此处仅保证切换根页表时 TLB
//! 一致性。

use core::arch::asm;

const SATP_ASID_SHIFT: usize = 44;
const SATP_ASID_MASK: usize = 0xffffusize << SATP_ASID_SHIFT;

unsafe extern "C" {
    static mut __wateros_riscv_asid_enabled: usize;
}

/// RISC-V Sv39 分页控制原语。
pub struct Riscv64Paging;

impl Riscv64Paging {
    #[inline]
    pub fn flush_tlb_local(range: api_v0::paging::TlbFlushRange) {
        unsafe {
            match range {
                api_v0::paging::TlbFlushRange::Page { addr } => {
                    asm!("sfence.vma {0}, x0", in(reg) addr);
                }
                api_v0::paging::TlbFlushRange::AddressSpace { token } => {
                    let asid = (token & SATP_ASID_MASK) >> SATP_ASID_SHIFT;
                    asm!("sfence.vma x0, {0}", in(reg) asid);
                }
                api_v0::paging::TlbFlushRange::Range { .. }
                | api_v0::paging::TlbFlushRange::All => {
                    asm!("sfence.vma x0, x0");
                }
            }
        }
    }

    /// 读取当前地址空间 token；在 RISC-V Sv39 下即 `satp` 当前值。
    #[inline]
    pub fn active_address_space_token() -> usize {
        let value: usize;
        unsafe {
            asm!("csrr {0}, satp", out(reg) value);
        }
        value
    }

    /// 探测 `satp.ASID` 实际实现的 WARL 位数，并配置 trap 汇编的快速切换开关。
    ///
    /// 调用时当前 `satp` 必须指向可执行的有效页表；探测过程保持 MODE/PPN
    /// 不变，恢复原 token 后执行一次全量 fence。
    pub fn initialize_address_space_ids() -> usize {
        let original: usize;
        let observed: usize;
        unsafe {
            asm!("csrr {0}, satp", out(reg) original);
            let probe = original | SATP_ASID_MASK;
            asm!("csrw satp, {0}", in(reg) probe);
            asm!("csrr {0}, satp", out(reg) observed);
            asm!("csrw satp, {0}", in(reg) original);
            asm!("sfence.vma x0, x0");
        }
        let implemented = ((observed & SATP_ASID_MASK) >> SATP_ASID_SHIFT).count_ones() as usize;
        unsafe {
            core::ptr::write_volatile(
                core::ptr::addr_of_mut!(__wateros_riscv_asid_enabled),
                usize::from(implemented != 0),
            );
        }
        implemented
    }

    /// 激活指定地址空间 token；在 RISC-V Sv39 下即写入 `satp` 并执行全局
    /// `sfence.vma`。
    #[inline]
    pub fn activate_address_space_token_and_flush(token: usize) {
        unsafe {
            asm!("csrw satp, {0}", in(reg) token);
            asm!("sfence.vma x0, x0");
        }
    }

    /// 刷新当前地址空间下的地址翻译缓存；在 RISC-V 下即全局 `sfence.vma`。
    #[inline]
    pub fn flush_address_space_translations() {
        Self::flush_tlb_local(api_v0::paging::TlbFlushRange::All);
    }

    /// RISC-V 下 MMU 由 OpenSBI 在 M 态管理，S 态不直接操作 MMU 使能位；no-op。
    #[inline]
    pub fn init_paging_disable_mmu() {}

    /// RISC-V 下 `satp` 的 MODE 字段决定分页模式；no-op（与 LoongArch
    /// 接口保持对称）。
    #[inline]
    pub fn enable_paging() {}
}
