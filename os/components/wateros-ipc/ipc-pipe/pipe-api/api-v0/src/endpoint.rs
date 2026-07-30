//! fd 表可见的 pipe 端点契约。

use crate::{PipeError, PipeResult};

/// pipe 端点方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeEndpointKind {
    /// 只读端点。
    Read,
    /// 只写端点。
    Write,
}

/// 可放入 fd 表的 pipe 端点契约。
///
/// `DATA:` 每个端点有独立的方向、状态标志与一次性关闭生命周期；clone 必须增加底层对应端
/// 的引用，最后一次 close/drop 才改变 pipe 的对端可见状态。
pub trait PipeEndpointOps {
    /// 创建一对读/写端点。
    fn pair(nonblocking: bool) -> (Self, Self)
    where
        Self: Sized;

    /// 端点方向。
    fn kind(&self) -> PipeEndpointKind;

    /// 是否按非阻塞语义执行。
    fn nonblocking(&self) -> bool;

    /// 从读端读取。
    fn read(&self, out: &mut [u8]) -> PipeResult<usize>;

    /// 从写端写入。
    fn write(&self, input: &[u8]) -> PipeResult<usize>;

    /// 显式关闭该端点。
    fn close(&self);

    /// 非阻塞查询 poll 就绪位（与请求的 `events` 掩码相交）。
    fn poll_revents(&self, events: i16) -> PipeResult<i16> {
        let _ = events;
        Ok(0)
    }

    /// 在 pipe 等待队列上带超时阻塞；`still_waiting` 为假时结束（供多 fd poll 重扫）。
    fn poll_wait_for_ticks(
        &self,
        events: i16,
        timeout_ticks: u64,
        still_waiting: &mut dyn FnMut() -> bool,
    ) -> PipeResult<()> {
        let _ = (events, timeout_ticks, still_waiting);
        Ok(())
    }

    /// 从读端读取；方向错误时返回 [`PipeError::BrokenPipe`]。
    #[inline]
    fn read_checked(&self, out: &mut [u8]) -> PipeResult<usize> {
        if self.kind() != PipeEndpointKind::Read {
            return Err(PipeError::BrokenPipe);
        }
        self.read(out)
    }

    /// 从写端写入；方向错误时返回 [`PipeError::BrokenPipe`]。
    #[inline]
    fn write_checked(&self, input: &[u8]) -> PipeResult<usize> {
        if self.kind() != PipeEndpointKind::Write {
            return Err(PipeError::BrokenPipe);
        }
        self.write(input)
    }
}
