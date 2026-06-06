//! 内核全局页表 bring-up 与用户 ELF 装载相关的 **API 契约**（实现见 `mm-impl`）。
//!
//! 成功路径返回的 [`LoadedElf::satp`] 为 **RISC-V `satp` 寄存器编码值**（MODE/ASID/根表 PPN），由具体 `mm-impl` 填写；切换用户任务前须由 arch 层 `csrw satp` 并刷新 TLB。
//!
//! 根卷读错误使用 [`RootVolumeReadError`]，由 `mm-impl` 从具体 FS 错误映射而来，**不**依赖 `wateros-fs` API crate，以保持 mm-api 与文件系统实现解耦。

use crate::error::MmError;
use crate::executable::ExecResolveError;

/// 根卷只读访问错误（语义对齐常见 FS 错误，但不绑定 `wateros-fs-api` 类型）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootVolumeReadError {
    /// 根卷未挂载或句柄未就绪。
    NotMounted,
    /// 路径不存在。
    NotFound,
    /// 期望文件但目标非普通文件等。
    NotAFile,
    /// 路径非法或不符合实现约束。
    InvalidPath,
    /// 内容非合法 UTF-8（若实现走字符串路径）。
    NotUtf8,
    /// 操作不被当前实现支持。
    Unsupported,
    /// 底层块设备或驱动错误。
    Driver,
    /// 卷元数据或结构损坏。
    Corrupt,
    /// 通用 I/O 失败。
    Io,
}

/// 根卷内可选默认用户 ELF 路径（固定 bring-up 或工具链使用）；正式测试盘以 shell 脚本为主时镜像可不包含此文件。
pub const DEFAULT_USER_ELF_PATH: &str = "/elf/user.elf";

/// ELF 装载或 MM 操作失败原因（实现可将解析错误归并到 `Parse`）。
#[derive(Debug)]
pub enum LoadElfError {
    /// 未挂载根文件系统或不可用。
    NoRootFs,
    /// 从根卷读取 ELF 失败（由 mm-impl 从 FS 错误映射）。
    RootVolume(RootVolumeReadError),
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

/// ELF 装载或 shebang 脚本解析失败（exec/spawn 统一入口）。
#[derive(Debug)]
pub enum LoadProgramError {
    /// 解释器 ELF 装载或 MM 失败。
    Elf(LoadElfError),
    /// 非 ELF 脚本的 shebang 解析失败。
    Script(ExecResolveError),
}

/// [`crate::elf_user_stack::prepare_elf_user_stack`] 失败原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrepareUserStackError {
    /// 用户栈向下增长越界。
    StackOverflow,
    /// 写入用户栈失败（未映射或权限不足）。
    AccessViolation,
    /// `LoadedElf::user_aspace_ptr` 为 0，无法访问用户地址空间。
    NoUserAspace,
}

/// 从根 FS 装载后的用户 ELF 视图（独立用户地址空间，**4 KiB 页**；具体分页格式由 mm-impl 决定）。
///
/// 虚拟地址区间与栈顶等由实现固定或计算；用户入口在 `entry_pc`，须在 **`satp` 已安装且 U 态可执行映射** 下跳转到该 PC。
pub struct LoadedElf {
    /// 首次进入用户态的入口虚拟地址；动态 ELF 时为解释器入口。
    pub entry_pc: usize,
    /// 主程序入口虚拟地址；用于 auxv `AT_ENTRY`。
    pub program_entry: usize,
    /// 动态解释器装载基址；无解释器时为 0，用于 auxv `AT_BASE`。
    pub interp_base: usize,
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
    /// 用户地址空间对象指针（**仅 `impl-sv39` 路径**；`brk`/`mmap` 等 syscall 通过此指针修改页表；`0` 表示无）。
    pub user_aspace_ptr: usize,
    /// 初始 program break（堆尾虚拟地址，与 `brk(0)` 初值一致）。
    pub brk_start: usize,
    /// 装载完成时的 `brk` 当前值（与 `brk_start` 相同直至首次扩展）。
    pub brk_current: usize,
    /// `brk` 允许增长的上限虚拟地址（不含）。
    pub brk_max: usize,
    /// 匿名 `mmap` 区 bump 起点（日志与调试；实现以页表内游标为准）。
    pub mmap_arena_base: usize,
    /// 用户 auxv `AT_PHDR`：程序头表在用户地址空间中的虚拟地址。
    pub phdr_va: usize,
    /// 用户 auxv `AT_PHNUM`：程序头数量。
    pub phnum: usize,
    /// 用户 auxv `AT_PHENT`：单个程序头字节大小。
    pub phentsize: usize,
}
