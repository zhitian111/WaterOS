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

    pub fn init(_start_ppn: usize, _end_ppn: usize, _ram_end_exclusive: usize) {}

    #[inline]
    pub fn kernel_satp() -> usize {
        0
    }

    pub fn map_identity_range_user(_start: VirtAddr, _end: VirtAddr, _perm: PagePerm) {}

    pub fn ensure_user_execute_for_kernel_va(_va: usize) {}

    pub fn map_anon_range_user(_start: VirtAddr, _end: VirtAddr, _perm: PagePerm) {}

    pub fn from_elf_path(_path: &str) -> Result<LoadedElf, LoadElfError> {
        Err(LoadElfError::BadClass)
    }
}
