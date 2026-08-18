//! MM 错误类型与 [`MmResult`]；供 syscall、装载器与 `mm-impl` 统一返回。

use frame_alloctor_api_v0::FrameAllocError;

/// MM 语义错误（用于 mm-api 与 syscall/loader 协作）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmError {
    /// 物理帧或内部资源耗尽。
    OutOfMemory,
    /// 地址/范围非法、发生算术溢出，或未满足实现要求的页对齐。
    InvalidAddress,
    /// 目标虚拟页已存在映射。
    AlreadyMapped,
    /// 目标虚拟页无映射或中间节点不符合预期。
    NotMapped,
    /// 权限不足或未映射导致的访问语义失败（由 `UserMemoryOps` 等返回）。
    AccessViolation,
    /// 当前实现不支持该操作（如非 4 KiB 页路径）；调用者不得把它转换成伪成功。
    Unsupported,

    /// 来自物理帧分配器的错误；`From` 转换目前归并为常见 MM 语义错误，此变体为扩展实现保留。
    FrameAlloc(FrameAllocError),
}

/// `Result` 别名，错误类型为 [`MmError`]。
pub type MmResult<T> = core::result::Result<T, MmError>;

/// 将帧分配器错误并入 MM 语义错误（供 `?` 在 syscall/impl 路径使用）。
impl From<FrameAllocError> for MmError {
    #[inline]
    fn from(value: FrameAllocError) -> Self {
        match value {
            FrameAllocError::OutOfMemory => MmError::OutOfMemory,
            FrameAllocError::InvalidFrame => MmError::InvalidAddress,
            FrameAllocError::Unsupported => MmError::Unsupported,
        }
    }
}
