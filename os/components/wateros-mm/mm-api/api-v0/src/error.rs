//! MM 错误类型与 [`MmResult`]；供 syscall、装载器与 `mm-impl` 统一返回。

use frame_alloctor_api_v0::FrameAllocError;

/// MM 语义错误（用于 mm-api 与 syscall/loader 协作）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmError {
    /// 物理帧或内部资源耗尽。
    OutOfMemory,
    /// 地址/范围非法或未对齐到实现要求。
    InvalidAddress,
    /// 目标虚拟页已存在映射。
    AlreadyMapped,
    /// 目标虚拟页无映射或中间节点不符合预期。
    NotMapped,
    /// 权限不足或未映射导致的访问语义失败（由 `UserMemoryOps` 等返回）。
    AccessViolation,
    /// 当前实现不支持该操作（如非 4K 大页路径）。
    Unsupported,

    /// 来自物理帧分配器的错误（会在语义上尽量映射到上层常见错误）。
    FrameAlloc(FrameAllocError),
}

/// `Result` 别名，错误类型为 [`MmError`]。
pub type MmResult<T> = core::result::Result<T, MmError>;

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

