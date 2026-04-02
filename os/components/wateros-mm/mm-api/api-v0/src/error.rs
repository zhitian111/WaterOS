use frame_alloctor_api_v0::FrameAllocError;

/// MM 语义错误（用于 mm-api 与 syscall/loader 协作）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmError {
    OutOfMemory,
    InvalidAddress,
    AlreadyMapped,
    NotMapped,
    AccessViolation,
    Unsupported,

    /// 来自物理帧分配器的错误（会在语义上尽量映射到上层常见错误）。
    FrameAlloc(FrameAllocError),
}

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

