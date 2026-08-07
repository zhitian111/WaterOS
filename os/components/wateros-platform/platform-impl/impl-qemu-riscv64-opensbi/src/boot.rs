//! OpenSBI 传入的启动参数及其类型化视图。

use api_v0::boot::PlatformBootArgs;

#[derive(Debug, Clone, Copy)]
/// OpenSBI 交给内核入口的原始 `a0/a1` 参数。
///
/// BOOT_CONTRACT: QEMU RISC-V `virt` 中分别约定为 hart id 与 DTB 物理地址；在
/// platform/MM 明确建立映射前，不能把 `arg1` 解释为可直接解引用的虚拟地址。
pub struct QEMURiscv64OpenSBIBootArgs {
    /// OpenSBI `a0`，当前映射为逻辑 CPU/hart id。
    arg0: usize,
    /// OpenSBI `a1`，当前映射为 DTB 物理地址。
    arg1: usize,
}

impl PlatformBootArgs for QEMURiscv64OpenSBIBootArgs {
    #[inline]
    fn arg0(&self) -> Option<usize> {
        Some(self.arg0)
    }
    #[inline]
    fn arg1(&self) -> Option<usize> {
        Some(self.arg1)
    }
}

impl QEMURiscv64OpenSBIBootArgs {
    /// 从 arch 启动入口保存的寄存器值构造参数包；不验证 DTB 内容。
    #[inline]
    pub fn new(arg0: usize, arg1: usize) -> Self {
        Self { arg0, arg1 }
    }
}

pub use QEMURiscv64OpenSBIBootArgs as BootArgs;
