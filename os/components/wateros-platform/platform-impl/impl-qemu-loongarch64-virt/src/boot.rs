//! LoongArch QEMU `virt` 固件传入的参数及其类型化视图。

use api_v0::boot::PlatformBootArgs;

#[derive(Debug, Clone, Copy)]
/// LoongArch QEMU 入口透传的三项原始参数。
///
/// BOOT_CONTRACT: 该 profile 当前不把这三项绑定为固定 ABI 语义；使用者应在真正
/// 需要某项前先由 machine/firmware 文档确认，避免照搬 RISC-V 的 hart/DTB 约定。
pub struct QEMULoongArch64VirtBootArgs {
    arg0: usize,
    arg1: usize,
    arg2: usize,
}

impl QEMULoongArch64VirtBootArgs {
    /// 从 arch 启动入口保存的参数构造，不做地址或 CPU 编号校验。
    #[inline]
    pub const fn new(arg0: usize, arg1: usize, arg2: usize) -> Self {
        Self { arg0, arg1, arg2 }
    }
}

impl PlatformBootArgs for QEMULoongArch64VirtBootArgs {
    #[inline]
    fn arg0(&self) -> Option<usize> {
        Some(self.arg0)
    }
    #[inline]
    fn arg1(&self) -> Option<usize> {
        Some(self.arg1)
    }
    #[inline]
    fn arg2(&self) -> Option<usize> {
        Some(self.arg2)
    }
}

#[derive(Debug, Clone, Copy)]
/// 原始参数的命名视图；保留公开字段以供当前 LoongArch 启动代码逐步消费。
pub struct QEMULoongArch64VirtBootContext {
    pub arg0: usize,
    pub arg1: usize,
    pub arg2: usize,
}

impl From<QEMULoongArch64VirtBootArgs> for QEMULoongArch64VirtBootContext {
    /// 无损转换，不赋予参数额外语义。
    #[inline]
    fn from(value: QEMULoongArch64VirtBootArgs) -> Self {
        Self {
            arg0: value.arg0,
            arg1: value.arg1,
            arg2: value.arg2,
        }
    }
}

pub use QEMULoongArch64VirtBootArgs as BootArgs;
pub use QEMULoongArch64VirtBootContext as BootContext;
