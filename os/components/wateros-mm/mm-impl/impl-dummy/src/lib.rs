//! `mm-impl` 桩：无 Sv39、无真实 `satp`；用于未启用 `wateros-mm` 的 `impl-sv39` 的构建。
//!
//! `kernel_mm_impl::from_elf_path` 等返回固定错误，避免链接真实 FS/页表路径。

#![no_std]

use api_v0::addr::VirtAddr;
use api_v0::kernel_bringup::{LoadElfError, LoadedElf};
use api_v0::perm::PagePerm;

/// 无 Sv39 / 非 QEMU bring-up 时的桩实现；由 `wateros-mm` 聚合为 `mm::kernel_mm`。
pub mod kernel_mm_impl {
    use super::*;

    /// 空操作；不安装 `satp`、不建立映射。启用 `impl-sv39` 后由真实实现替换。
    pub fn init(_start_ppn: usize, _end_ppn: usize, _ram_end_exclusive: usize) {}

    /// 恒为 `0`；调用方不得将其当作合法 `satp` 写入硬件。
    #[inline]
    pub fn kernel_satp() -> usize {
        0
    }

    /// 空操作；桩不修改任何页表。
    pub fn map_identity_range_user(_start: VirtAddr, _end: VirtAddr, _perm: PagePerm) {}

    /// 空操作。
    pub fn ensure_user_execute_for_kernel_va(_va: usize) {}

    /// 空操作。
    pub fn map_anon_range_user(_start: VirtAddr, _end: VirtAddr, _perm: PagePerm) {}

    /// 固定返回 [`LoadElfError::BadClass`]，避免在未启用 Sv39 时链接 FS/ELF 路径。
    pub fn from_elf_path(_path: &str) -> Result<LoadedElf, LoadElfError> {
        Err(LoadElfError::BadClass)
    }
}
