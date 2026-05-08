//! 用户缓冲区访问：通常在 **trap 处理 / syscall 内核路径** 中调用，此时应已持有正确的地址空间上下文（或能推导当前任务页表）。

use crate::addr::VirtAddr;
use crate::error::MmResult;

/// 用户态内存访问（用于 syscall、glibc、文件系统等）。
///
/// 实现应当：
/// - 将 `VirtAddr` 翻译到 **当前** 用户地址空间的物理映射（页大小与 [`crate::addr::PAGE_SIZE`] 一致）；
/// - 处理跨页拷贝与权限校验（未映射页不得静默成功）；
/// - 对未映射/越权返回合适的 [`crate::error::MmError`]。
pub trait UserMemoryOps {
    /// 从用户虚拟地址 `src` 起读取，写入 `dst`；可跨页，遇未映射或权限不足返回错误。
    ///
    /// 成功返回实际拷贝的字节数（当前 API 与全量成功语义一致时可等于 `dst.len()`，以实现为准）。
    fn copy_from_user(&self, dst: &mut [u8], src: VirtAddr) -> MmResult<usize>;

    /// 将 `src` 写入用户缓冲区 `dst` 起始处；可跨页。
    fn copy_to_user(&self, dst: VirtAddr, src: &[u8]) -> MmResult<usize>;
}

