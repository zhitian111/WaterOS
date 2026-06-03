//! 内核内部 pipe 对象契约。

use crate::{PipeResult, DEFAULT_PIPE_CAPACITY};

/// 内核内部 ring-buffer pipe 契约。
pub trait KernelPipe {
    /// 创建指定容量的 pipe。
    fn with_capacity(capacity: usize) -> PipeResult<Self>
    where
        Self: Sized;

    /// 创建默认容量的 pipe。
    #[inline]
    fn new() -> Self
    where
        Self: Sized,
    {
        Self::with_capacity(DEFAULT_PIPE_CAPACITY)
            .expect("default pipe capacity must be valid")
    }

    /// 返回缓冲区容量。
    fn capacity(&self) -> usize;

    /// 返回当前已缓冲字节数。
    fn len(&self) -> usize;

    /// 非阻塞读取；空且写端未关闭时返回 [`PipeError::WouldBlock`]。
    fn try_read(&self, out: &mut [u8]) -> PipeResult<usize>;

    /// 阻塞读取；空且写端关闭时返回 EOF（`Ok(0)`）。
    fn read(&self, out: &mut [u8]) -> PipeResult<usize>;

    /// 非阻塞写入；满且读端仍打开时返回 [`PipeError::WouldBlock`]。
    fn try_write(&self, input: &[u8]) -> PipeResult<usize>;

    /// 阻塞写入，尽量写完整个输入缓冲。
    fn write(&self, input: &[u8]) -> PipeResult<usize>;

    /// 关闭读端并唤醒可能阻塞的写者。
    fn close_read(&self);

    /// 关闭写端并唤醒可能阻塞的读者。
    fn close_write(&self);

    /// 读端 poll 就绪位（`POLLIN` / `POLLHUP` 等，与 Linux 语义对齐的原始位）。
    fn poll_revents_read(&self) -> i16 {
        let _ = self;
        0
    }

    /// 写端 poll 就绪位（`POLLOUT` / `POLLHUP` 等）。
    fn poll_revents_write(&self) -> i16 {
        let _ = self;
        0
    }
}
