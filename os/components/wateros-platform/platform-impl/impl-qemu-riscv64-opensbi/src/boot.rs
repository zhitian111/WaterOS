//! OpenSBI 传入的启动参数及其类型化视图。

use api_v0::boot::PlatformBootArgs;

#[derive(Debug, Clone, Copy)]
/// OpenSBI 交给内核入口的原始 `a0/a1` 参数。
///
/// BOOT_CONTRACT: QEMU RISC-V `virt` 中分别约定为 hart id 与 DTB 物理地址；在
/// 转换为 [`BootContext`] 前不能把它们解释为可直接解引用的虚拟地址。
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

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
/// RISC-V QEMU 启动参数的类型化视图。
///
/// 该类型目前只用于启动阶段交接；CPU-local 和 DTB 解析状态不应塞入这里。
pub struct QEMURiscv64OpenSBIBootContext {
    hart_id: base::cpu::CPUHartID,
    dtb_pa: base::boot::DTBPA,
}

impl From<QEMURiscv64OpenSBIBootArgs> for QEMURiscv64OpenSBIBootContext {
    /// 将两项必填固件参数转为类型化值；缺失说明入口 ABI 被破坏，直接 panic。
    #[inline]
    fn from(value: QEMURiscv64OpenSBIBootArgs) -> Self {
        let hart_id = value
            .arg0()
            .expect("OpenSBI boot arg0 is absent");
        let dtb_pa = value
            .arg1()
            .expect("OpenSBI boot arg1 is absent");
        Self {
            hart_id,
            dtb_pa: base::addr::BasePhysAddr { val: dtb_pa },
        }
    }
}

pub use QEMURiscv64OpenSBIBootArgs as BootArgs;
pub use QEMURiscv64OpenSBIBootContext as BootContext;
