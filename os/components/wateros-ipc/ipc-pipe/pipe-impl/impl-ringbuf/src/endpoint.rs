//! fd 表 pipe 端点实现。
//!
//! `DATA:` 每个端点持有同一个 `Arc<Pipe>`；dup/fork clone 共享
//! `O_NONBLOCK`/`O_DIRECT`，但每个 wrapper 保留独立 close 位。端点引用数在
//! clone/close/drop 时同步到底层 `Pipe`。

extern crate alloc;

use alloc::boxed::Box;
use alloc::sync::Arc;
use api_v0::{
    PipeEndpointKind, PipeEndpointOps, PipeError, PipeReadLease, PipeResult,
};
use core::cell::Cell;
use core::sync::atomic::{AtomicBool, Ordering};
use waitqueue::TaskWaitResult;

use crate::kernel_pipe::Pipe;

/// Shared backing object for a filesystem FIFO. It owns no endpoint reference,
/// so EOF and `ENXIO` depend only on currently open file descriptions.
pub struct NamedPipe {
    pipe: Arc<Pipe>,
}

impl NamedPipe {
    pub fn new() -> Self {
        Self {
            pipe: Arc::new(Pipe::new_named()),
        }
    }

    pub fn open_read(&self, nonblocking: bool) -> PipeResult<PipeEndpoint> {
        let endpoint = PipeEndpoint::open(self.pipe.clone(), PipeEndpointKind::Read, nonblocking);
        if !nonblocking {
            self.pipe.wait_for_writer()?;
        }
        Ok(endpoint)
    }

    pub fn open_write(&self, nonblocking: bool) -> PipeResult<PipeEndpoint> {
        if nonblocking && !self.pipe.has_readers() {
            return Err(PipeError::BrokenPipe);
        }
        let endpoint = PipeEndpoint::open(self.pipe.clone(), PipeEndpointKind::Write, nonblocking);
        if !nonblocking {
            self.pipe.wait_for_reader()?;
        }
        Ok(endpoint)
    }
}

impl Default for NamedPipe {
    fn default() -> Self {
        Self::new()
    }
}

/// 可放入 fd table 的 pipe 端点。
///
/// `INVARIANT:` `closed == true` 后不得再次减少 pipe 引用；这让显式 close 与析构幂等。
pub struct PipeEndpoint {
    pipe: Arc<Pipe>,
    kind: PipeEndpointKind,
    nonblocking: Arc<AtomicBool>,
    direct: Arc<AtomicBool>,
    /// 每个端点实例只释放一次引用；显式 `close` 与析构可以安全共存。
    closed: Cell<bool>,
}

impl Clone for PipeEndpoint {
    fn clone(&self) -> Self {
        let closed = self.closed.get();
        if !closed {
            match self.kind {
                PipeEndpointKind::Read => self
                    .pipe
                    .acquire_read(),
                PipeEndpointKind::Write => self
                    .pipe
                    .acquire_write(),
            }
        }
        Self {
            pipe: self.pipe.clone(),
            kind: self.kind,
            nonblocking: self.nonblocking.clone(),
            direct: self.direct.clone(),
            closed: Cell::new(closed),
        }
    }
}

impl Drop for PipeEndpoint {
    fn drop(&mut self) {
        self.release_once();
    }
}

impl PipeEndpoint {
    fn open(pipe: Arc<Pipe>, kind: PipeEndpointKind, nonblocking: bool) -> Self {
        match kind {
            PipeEndpointKind::Read => pipe.acquire_read(),
            PipeEndpointKind::Write => pipe.acquire_write(),
        }
        Self {
            pipe,
            kind,
            nonblocking: Arc::new(AtomicBool::new(nonblocking)),
            direct: Arc::new(AtomicBool::new(false)),
            closed: Cell::new(false),
        }
    }

    /// 创建一对读/写端点。
    pub fn pair(nonblocking: bool) -> (Self, Self) {
        PipeEndpointOps::pair(nonblocking)
    }

    /// 创建一对读/写端点，并保留 pipe 状态标志。
    pub fn pair_with_flags(nonblocking: bool, direct: bool) -> (Self, Self) {
        let (read, write) = <Self as PipeEndpointOps>::pair(nonblocking);
        read.set_direct(direct);
        write.set_direct(direct);
        (read, write)
    }

    /// 端点方向。
    pub const fn kind(&self) -> PipeEndpointKind {
        self.kind
    }

    /// 是否按非阻塞语义执行。
    pub fn nonblocking(&self) -> bool {
        self.nonblocking
            .load(Ordering::Acquire)
    }

    /// 切换非阻塞模式（`fcntl(F_SETFL)`）。
    pub fn set_nonblocking(&self, value: bool) {
        self.nonblocking
            .store(value, Ordering::Release);
    }

    /// 是否按 Linux `O_DIRECT` pipe packet-mode 标记打开。
    #[inline]
    pub fn direct(&self) -> bool {
        self.direct.load(Ordering::Acquire)
    }

    /// 切换 `O_DIRECT` pipe 状态位（`pipe2` / `fcntl(F_SETFL)`）。
    pub fn set_direct(&self, value: bool) {
        self.direct
            .store(value, Ordering::Release);
    }

    /// 返回底层 pipe 缓冲区容量。
    #[inline]
    pub fn pipe_capacity(&self) -> usize {
        self.pipe.capacity()
    }

    /// 返回底层 pipe 当前已缓冲字节数。
    #[inline]
    pub fn pipe_len(&self) -> usize {
        self.pipe.len()
    }

    /// 调整底层 pipe 缓冲区容量（`fcntl(F_SETPIPE_SZ)`）。
    pub fn set_pipe_capacity(&self, capacity: usize) -> PipeResult<usize> {
        self.ensure_open()?;
        self.pipe
            .set_capacity(capacity)
    }

    /// 从读端读取。
    pub fn read(&self, out: &mut [u8]) -> PipeResult<usize> {
        PipeEndpointOps::read(self, out)
    }

    /// 等待可读数据并建立锁外 read reservation。
    pub fn acquire_read_lease(&self, max_len: usize) -> PipeResult<Box<dyn PipeReadLease>> {
        PipeEndpointOps::acquire_read_lease(self, max_len)
    }

    /// 从写端写入。
    pub fn write(&self, input: &[u8]) -> PipeResult<usize> {
        PipeEndpointOps::write(self, input)
    }

    /// 显式关闭该端点。
    pub fn close(&self) {
        PipeEndpointOps::close(self);
    }

    /// `FLOW:` 端点的唯一释放点；最后一个同方向端点会唤醒对端等待者。
    fn release_once(&self) {
        if self
            .closed
            .replace(true)
        {
            return;
        }
        match self.kind {
            PipeEndpointKind::Read => self
                .pipe
                .release_read(),
            PipeEndpointKind::Write => self
                .pipe
                .release_write(),
        }
    }

    /// 关闭后的端点可能仍被某个内核对象暂时持有，但不再代表有效 fd。
    fn ensure_open(&self) -> PipeResult<()> {
        if self.closed.get() {
            Err(PipeError::Closed)
        } else {
            Ok(())
        }
    }
}

impl PipeEndpointOps for PipeEndpoint {
    fn pair(nonblocking: bool) -> (Self, Self) {
        let pipe = Arc::new(Pipe::new());
        pipe.acquire_read();
        pipe.acquire_write();
        (
            Self {
                pipe: pipe.clone(),
                kind: PipeEndpointKind::Read,
                nonblocking: Arc::new(AtomicBool::new(nonblocking)),
                direct: Arc::new(AtomicBool::new(false)),
                closed: Cell::new(false),
            },
            Self {
                pipe,
                kind: PipeEndpointKind::Write,
                nonblocking: Arc::new(AtomicBool::new(nonblocking)),
                direct: Arc::new(AtomicBool::new(false)),
                closed: Cell::new(false),
            },
        )
    }

    fn kind(&self) -> PipeEndpointKind {
        self.kind
    }

    fn nonblocking(&self) -> bool {
        self.nonblocking
            .load(Ordering::Acquire)
    }

    fn read(&self, out: &mut [u8]) -> PipeResult<usize> {
        self.ensure_open()?;
        if self.kind != PipeEndpointKind::Read {
            return Err(PipeError::BrokenPipe);
        }
        if self
            .nonblocking
            .load(Ordering::Acquire)
        {
            self.pipe
                .try_read(out)
        } else {
            self.pipe.read(out)
        }
    }

    fn acquire_read_lease(&self, max_len: usize) -> PipeResult<Box<dyn PipeReadLease>> {
        self.ensure_open()?;
        if self.kind != PipeEndpointKind::Read {
            return Err(PipeError::BrokenPipe);
        }
        self.pipe.acquire_read_lease(max_len, self.nonblocking())
    }

    fn write(&self, input: &[u8]) -> PipeResult<usize> {
        self.ensure_open()?;
        if self.kind != PipeEndpointKind::Write {
            return Err(PipeError::BrokenPipe);
        }
        if self
            .nonblocking
            .load(Ordering::Acquire)
        {
            self.pipe
                .try_write_mode(input, self.direct())
        } else {
            self.pipe.write_mode(input, self.direct())
        }
    }

    fn close(&self) {
        self.release_once();
    }

    fn poll_revents(&self, events: i16) -> PipeResult<i16> {
        self.ensure_open()?;
        const POLLIN: i16 = 0x001;
        const POLLOUT: i16 = 0x004;
        let raw = match self.kind {
            PipeEndpointKind::Read => self
                .pipe
                .poll_revents_read(),
            PipeEndpointKind::Write => self
                .pipe
                .poll_revents_write(),
        };
        let mut out = 0i16;
        if events & POLLIN != 0 && raw & POLLIN != 0 {
            out |= POLLIN;
        }
        if events & POLLOUT != 0 && raw & POLLOUT != 0 {
            out |= POLLOUT;
        }
        if raw & (0x008 | 0x010) != 0 {
            out |= raw & (0x008 | 0x010);
        }
        Ok(out)
    }

    /// `FLOW:` 仅在端点方向与请求事件匹配时把 poll 交给对应 pipe 等待队列。
    fn poll_wait_for_ticks(
        &self,
        events: i16,
        timeout_ticks: u64,
        still_waiting: &mut dyn FnMut() -> bool,
    ) -> PipeResult<()> {
        self.ensure_open()?;
        const POLLIN: i16 = 0x001;
        const POLLOUT: i16 = 0x004;
        if events & POLLIN != 0 && self.kind == PipeEndpointKind::Read {
            let result = self
                .pipe
                .poll_wait_read_for_ticks(timeout_ticks, still_waiting);
            if result == TaskWaitResult::Interrupted {
                return Err(PipeError::Interrupted);
            }
        }
        if events & POLLOUT != 0 && self.kind == PipeEndpointKind::Write {
            let result = self
                .pipe
                .poll_wait_write_for_ticks(timeout_ticks, still_waiting);
            if result == TaskWaitResult::Interrupted {
                return Err(PipeError::Interrupted);
            }
        }
        Ok(())
    }
}
