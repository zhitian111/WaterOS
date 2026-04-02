use crate::addr::VirtAddr;
use crate::error::MmResult;

/// 用户态内存访问（用于 syscall、glibc、文件系统等）。
///
/// 实现应当：
/// - 将 `VirtAddr` 翻译到当前地址空间的物理映射；
/// - 处理跨页拷贝与权限校验；
/// - 对未映射/越权返回合适的 `MmError`。
pub trait UserMemoryOps {
    /// 从用户虚拟地址拷贝到内核缓冲区。
    /// 返回实际拷贝字节数；失败则返回 `MmError`。
    fn copy_from_user(&self, dst: &mut [u8], src: VirtAddr) -> MmResult<usize>;

    /// 将内核缓冲区写入用户虚拟地址。
    /// 返回实际写入字节数；失败则返回 `MmError`。
    fn copy_to_user(&self, dst: VirtAddr, src: &[u8]) -> MmResult<usize>;
}

