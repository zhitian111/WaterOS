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
    /// 从用户虚拟地址拷贝到内核缓冲区。
    /// 返回实际拷贝字节数；失败则返回 `MmError`。
    fn copy_from_user(&self, dst: &mut [u8], src: VirtAddr) -> MmResult<usize>;

    /// 将内核缓冲区写入用户虚拟地址。
    /// 返回实际写入字节数；失败则返回 `MmError`。
    fn copy_to_user(&self, dst: VirtAddr, src: &[u8]) -> MmResult<usize>;
}

