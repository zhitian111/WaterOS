//! LoongArch QEMU `virt` 固件传入的原始参数。

use api_v0::boot::PlatformBootArgs;

/// QEMU LoongArch `virt` 在 direct-kernel boot 时放置自动生成 DTB 的物理地址。
/// 该地址属于 machine profile，不是 LoongArch ISA 启动 ABI。
pub const DEVICE_TREE_PHYS_ADDR : usize = 0x0010_0000;

#[inline]
pub const fn device_tree_phys_addr(_arg0 : usize, _arg1 : usize, _arg2 : usize) -> usize {
    DEVICE_TREE_PHYS_ADDR
}

#[derive(Debug, Clone, Copy)]
/// LoongArch QEMU 入口透传的三项原始参数。
///
/// BOOT_CONTRACT: 该 profile 当前不把这三项绑定为固定 ABI 语义；使用者应在真正
/// 需要某项前先由 machine/firmware 文档确认，避免照搬 RISC-V 的 hart/DTB 约定。
pub struct QEMULoongArch64VirtBootArgs {
    arg0 : usize,
    arg1 : usize,
    arg2 : usize,
}

impl QEMULoongArch64VirtBootArgs {
    /// 从 arch 启动入口保存的参数构造，不做地址或 CPU 编号校验。
    #[inline]
    pub const fn new(arg0 : usize, arg1 : usize, arg2 : usize) -> Self { Self { arg0, arg1, arg2 } }
}

impl PlatformBootArgs for QEMULoongArch64VirtBootArgs {
    #[inline]
    fn arg0(&self) -> Option<usize> { Some(self.arg0) }
    #[inline]
    fn arg1(&self) -> Option<usize> { Some(self.arg1) }
    #[inline]
    fn arg2(&self) -> Option<usize> { Some(self.arg2) }
}

pub use QEMULoongArch64VirtBootArgs as BootArgs;
