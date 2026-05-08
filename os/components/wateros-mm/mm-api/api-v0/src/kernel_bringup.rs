//! 内核全局页表 bring-up 与用户 ELF 装载相关的 **API 契约**（实现见 `mm-impl`）。
//!
//! 成功路径返回的 [`LoadedElf::satp`] 为 **RISC-V `satp` 寄存器编码值**（MODE/ASID/根表 PPN），由具体 `mm-impl` 填写；切换用户任务前须由 arch 层 `csrw satp` 并刷新 TLB。

pub use fs_api_v0::FsError;

use crate::error::MmError;

/// 根卷内默认用户 ELF 路径（可按镜像修改）；与常见镜像中 `/elf/000_hello_world.elf` 一致。
pub const DEFAULT_USER_ELF_PATH: &str = "/elf/000_hello_world.elf";

/// ELF 装载或 MM 操作失败原因（实现可将解析错误归并到 `Parse`）。
#[derive(Debug)]
pub enum LoadElfError {
    /// 未挂载根文件系统或不可用。
    NoRootFs,
    /// 根 FS 读文件失败。
    Fs(FsError),
    /// 输入字节过短。
    TooSmall,
    /// 非 `\x7FELF`。
    BadMagic,
    /// 非 ELFCLASS64（当前仅支持 64 位）。
    BadClass,
    /// 非小端（当前仅支持 ELFDATA2LSB）。
    BadEndian,
    /// 非 `EM_RISCV`。
    BadMachine,
    /// 程序头/段布局等解析失败。
    Parse,
    /// 映射、分配帧等 MM 错误。
    Mm(MmError),
}

/// 将 [`MmError`] 提升为装载错误（页表/帧分配失败路径）。
impl From<MmError> for LoadElfError {
    fn from(e: MmError) -> Self {
        LoadElfError::Mm(e)
    }
}

/// 从根 FS 装载后的用户 ELF 视图（独立 Sv39 地址空间，**4 KiB 页**）。
///
/// 虚拟地址区间与栈顶等由实现固定或计算；用户入口在 `entry_pc`，须在 **`satp` 已安装且 U 态可执行映射** 下跳转到该 PC。
pub struct LoadedElf {
    /// 用户程序入口虚拟地址（通常为 ELF `e_entry`）。
    pub entry_pc: usize,
    /// 该地址空间对应的 `satp`（实现相关编码）。
    pub satp: usize,
    /// 用户栈下界（虚拟地址，含）。
    pub stack_bottom: usize,
    /// 用户栈上界（虚拟地址，常为对齐后的栈顶）。
    pub stack_top: usize,
    /// 映射镜像的最低虚拟地址。
    pub image_base: usize,
    /// 镜像虚拟范围大小（字节）。
    pub image_size: usize,
}
