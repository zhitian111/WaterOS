//! fd 表可见的 pipe 端点契约。

extern crate alloc;

use alloc::boxed::Box;

use crate::{PipeError, PipeResult};

/// pipe 端点方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeEndpointKind {
    /// 只读端点。
    Read,
    /// 只写端点。
    Write,
}

/// pipe reservation 根据 user-copy 结果产生的提交结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeReadFinish {
    /// stream 数据已提交指定前缀，或 packet 已完整交付。
    Bytes(usize),
    /// packet 发生部分 user-copy fault；packet 已按记录语义消费。
    Fault,
}

/// 在 pipe 锁外持有的稳定读取快照。
pub trait PipeReadLease: Send {
    fn bytes(&self) -> &[u8];

    /// `complete` 表示整个 staging 已复制；消耗 lease 后必须提交或取消 reservation。
    fn finish(
        self: Box<Self>,
        copied: usize,
        complete: bool,
    ) -> PipeResult<PipeReadFinish>;
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

    /// 在不持 fd 锁的调用路径中等待数据并建立 read reservation。
    fn acquire_read_lease(&self, max_len: usize) -> PipeResult<Box<dyn PipeReadLease>>;

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
