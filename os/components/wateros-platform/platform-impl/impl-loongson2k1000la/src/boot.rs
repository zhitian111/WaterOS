//! Loongson 2K1000LA 启动参数：PMON/uImage 不携带 UEFI 参数。
//!
//! 旧分支曾假设 UEFI ABI（a0=efi_boot/a1=cmdline/a2=EFI system table + DTB GUID），
//! 与参考实现（同款板，PMON + uImage）不符；此处改为 PMON 语义。DTB 由板级入口
//! 显式保存，不在此解析。

use api_v0::boot::PlatformBootArgs;

#[derive(Debug, Clone, Copy)]
pub struct Loongson2K1000BootArgs;

impl PlatformBootArgs for Loongson2K1000BootArgs {}

pub use Loongson2K1000BootArgs as BootArgs;

/// 返回当前保存的 DTB 物理基址（PMON 通常不提供；为 0 时内存用板级回退）。
pub fn device_tree_phys_addr() -> usize {
    crate::dtb::dtb_pa()
}
