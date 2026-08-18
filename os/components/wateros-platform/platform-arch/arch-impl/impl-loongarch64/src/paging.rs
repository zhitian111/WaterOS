//! LoongArch64 **分页控制原语**：通过 CSR.PGDL 读写根页表物理页号，
//! 通过 `invtlb` 指令刷新 TLB，以及 `CRMD.PG` 分页使能位控制。
//!
//! 页表内容构造与用户/内核映射策略在 `wateros-mm`；此处仅保证切换根页表时 TLB
//! 一致性与 MMU 使能/禁用。

use core::arch::asm;

/// CRMD 寄存器编号。
const CSR_CRMD: usize = 0x0;
/// PGDL 寄存器编号。
const CSR_PGDL: usize = 0x19;
/// ASID 寄存器编号。
const CSR_ASID: usize = 0x18;
const ASID_MASK: usize = 0x3ff;
/// 地址空间 token 中 ASID 的高位偏移。
const TOKEN_ASID_SHIFT: usize = 48;
/// 地址空间 token 中保存 PGDL 的低位掩码。
const TOKEN_PGDL_MASK: usize = (1usize << TOKEN_ASID_SHIFT) - 1;
/// CRMD.PG（bit 4）：分页使能。
const CRMD_PG: usize = 1 << 4;
/// CRMD.DA（bit 3）：直接地址翻译模式。
const CRMD_DA: usize = 1 << 3;

/// LoongArch64 分页控制原语。
pub struct LoongArch64Paging;

impl LoongArch64Paging {
    #[inline]
    pub fn initialize_address_space_ids() -> usize {
        10
    }

    #[inline]
    pub fn flush_tlb_local(_range: api_v0::paging::TlbFlushRange) {
        Self::invtlb_all();
    }

    #[inline]
    fn read_pgdl() -> usize {
        let value: usize;
        unsafe {
            asm!("csrrd {0}, {1}", out(reg) value, const CSR_PGDL);
        }
        value
    }

    #[inline]
    fn write_pgdl(pgdl: usize) {
        unsafe {
            asm!("csrwr {0}, {1}", in(reg) pgdl, const CSR_PGDL);
        }
    }

    #[inline]
    fn read_asid() -> usize {
        let value: usize;
        unsafe {
            asm!("csrrd {0}, {1}", out(reg) value, const CSR_ASID);
        }
        value & ASID_MASK
    }

    #[inline]
    fn write_asid(asid: usize) {
        unsafe {
            asm!("csrwr {0}, {1}", in(reg) (asid & ASID_MASK), const CSR_ASID);
        }
    }

    #[inline]
    const fn token_pgdl(token: usize) -> usize {
        token & TOKEN_PGDL_MASK
    }

    #[inline]
    const fn token_asid(token: usize) -> usize {
        (token >> TOKEN_ASID_SHIFT) & ASID_MASK
    }

    /// 全局 TLB 刷新。
    #[inline]
    fn invtlb_all() {
        unsafe {
            asm!("invtlb 0, $zero, $zero");
        }
    }

    #[inline]
    pub fn active_address_space_token() -> usize {
        Self::token_pgdl(Self::read_pgdl()) | (Self::read_asid() << TOKEN_ASID_SHIFT)
    }

    #[inline]
    pub fn activate_address_space_token_and_flush(token: usize) {
        Self::write_pgdl(Self::token_pgdl(token));
        Self::write_asid(Self::token_asid(token));
        Self::invtlb_all();
    }

    #[inline]
    pub fn flush_address_space_translations() {
        Self::flush_tlb_local(api_v0::paging::TlbFlushRange::All);
    }

    /// 关闭 MMU（CRMD.DA = 1, CRMD.PG = 0），使后续访存直接使用物理地址。
    /// 在构建内核页表前调用，避免固件页表无法覆盖全部 RAM 导致的页错误。
    ///
    /// 读取 CRMD 后清除 PG 位，用 `csrwr` + `inout(reg)` 确保写入不被优化。
    #[inline]
    pub fn init_paging_disable_mmu() {
        let mut crmd: usize;
        unsafe {
            asm!("csrrd {0}, {1}", out(reg) crmd, const CSR_CRMD);
        }
        let desired = (crmd | CRMD_DA) & !CRMD_PG;
        if crmd != desired {
            crmd = desired;
            unsafe {
                asm!("csrwr {0}, {1}", inout(reg) crmd => _, const CSR_CRMD);
            }
            Self::invtlb_all();
        }
    }

    /// 开启 MMU（CRMD.DA = 0, CRMD.PG = 1），与 `activate_address_space_token_and_flush`
    /// 配合完成内核页表切换。
    ///
    /// 读取 CRMD 后设置 PG 位，用 `csrwr` + `inout(reg)` 确保写入不被优化。
    #[inline]
    pub fn enable_paging() {
        let mut crmd: usize;
        unsafe {
            asm!("csrrd {0}, {1}", out(reg) crmd, const CSR_CRMD);
        }
        if (crmd & CRMD_PG) == 0 {
            crmd = (crmd | CRMD_PG) & !CRMD_DA;
            unsafe {
                asm!("csrwr {0}, {1}", inout(reg) crmd => _, const CSR_CRMD);
            }
        }
    }
}
