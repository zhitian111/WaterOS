//! LoongArch64 **分页占位**：满足 `wateros-platform-arch` facade 链接；接入真实 MMU
//! 后应实现根页表切换与 TLB 刷新，并与 `wateros-mm` 策略对齐。

/// LoongArch64 页表控制占位实现。
///
/// 当前仓库尚未提供 LoongArch 页表 impl；这些入口只保证 arch facade 可编译。
pub struct LoongArch64Paging;

impl LoongArch64Paging {
    #[inline]
    pub fn read_satp() -> usize {
        0
    }

    #[inline]
    pub fn write_satp_and_flush(_satp: usize) {}

    #[inline]
    pub fn sfence_vma_all() {}
}
