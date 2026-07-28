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
    fn copy_from_user(&self, dst : &mut [u8], src : VirtAddr) -> MmResult<usize>;

    /// 将 `src` 写入用户缓冲区 `dst` 起始处；可跨页。
    fn copy_to_user(&self, dst : VirtAddr, src : &[u8]) -> MmResult<usize>;

    /// 原子读取一个四字节对齐的用户 `u32`。
    fn atomic_load_u32(&self, src : VirtAddr) -> MmResult<u32>;

    /// 当用户 `u32` 等于 `expected` 时原子写入 `desired`，无论交换是否
    /// 成功都返回操作时观察到的旧值。
    fn atomic_compare_exchange_u32(&self,
                                   dst : VirtAddr,
                                   expected : u32,
                                   desired : u32)
                                   -> MmResult<u32>;

    /// 返回 shared futex 使用的映射身份。当前实现使用已翻译物理字地址，
    /// 因而同一共享页在不同进程、不同 VA 下仍能得到相同 key。
    fn shared_futex_key_u32(&self, src : VirtAddr) -> MmResult<usize>;
}
