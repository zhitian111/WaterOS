//! fd 表 pipe 端点实现。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::sync::Arc;
use core::cell::Cell;
use api_v0::{PipeEndpointKind, PipeEndpointOps, PipeError, PipeResult};
use waitqueue::TaskWaitResult;

use crate::kernel_pipe::Pipe;

/// 可放入 fd table 的 pipe 端点。
// 本结构代码由AI完成
pub struct PipeEndpoint {
    pipe: Arc<Pipe>,
    kind: PipeEndpointKind,
    nonblocking: Cell<bool>,
    direct: Cell<bool>,
    /// 每个端点实例只释放一次引用；显式 `close` 与析构可以安全共存。
    closed: Cell<bool>,
}

impl Clone for PipeEndpoint {
// 本方法代码由AI完成
    fn clone(&self) -> Self {
        let closed = self.closed.get();
        if !closed {
            match self.kind {
                PipeEndpointKind::Read => self.pipe.acquire_read(),
                PipeEndpointKind::Write => self.pipe.acquire_write(),
            }
        }
        Self {
            pipe: self.pipe.clone(),
            kind: self.kind,
            nonblocking: Cell::new(self.nonblocking.get()),
            direct: Cell::new(self.direct.get()),
            closed: Cell::new(closed),
        }
    }
}

impl Drop for PipeEndpoint {
    fn drop(&mut self) { self.release_once(); }
}

impl PipeEndpoint {
    /// 创建一对读/写端点。
// 本方法代码由AI完成
    pub fn pair(nonblocking: bool) -> (Self, Self) {
        PipeEndpointOps::pair(nonblocking)
    }

    /// 创建一对读/写端点，并保留 pipe 状态标志。
// 本方法代码由AI完成
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
        self.nonblocking.get()
    }

    /// 切换非阻塞模式（`fcntl(F_SETFL)`）。
// 本方法代码由AI完成
    pub fn set_nonblocking(&self, value: bool) {
        self.nonblocking.set(value);
    }

    /// 是否按 Linux `O_DIRECT` pipe packet-mode 标记打开。
    #[inline]
    pub fn direct(&self) -> bool {
        self.direct.get()
    }

    /// 切换 `O_DIRECT` pipe 状态位（`pipe2` / `fcntl(F_SETFL)`）。
// 本方法代码由AI完成
    pub fn set_direct(&self, value: bool) {
        self.direct.set(value);
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
// 本方法代码由AI完成
    pub fn set_pipe_capacity(&self, capacity: usize) -> PipeResult<usize> {
        self.pipe.set_capacity(capacity)
    }

    /// 从读端读取。
// 本方法代码由AI完成
    pub fn read(&self, out: &mut [u8]) -> PipeResult<usize> {
        PipeEndpointOps::read(self, out)
    }

    /// 从写端写入。
// 本方法代码由AI完成
    pub fn write(&self, input: &[u8]) -> PipeResult<usize> {
        PipeEndpointOps::write(self, input)
    }

    /// 显式关闭该端点。
// 本方法代码由AI完成
    pub fn close(&self) {
        PipeEndpointOps::close(self);
    }

    fn release_once(&self) {
        if self.closed.replace(true) {
            return;
        }
        match self.kind {
            PipeEndpointKind::Read => self.pipe.release_read(),
            PipeEndpointKind::Write => self.pipe.release_write(),
        }
    }
}

impl PipeEndpointOps for PipeEndpoint {
// 本方法代码由AI完成
    fn pair(nonblocking: bool) -> (Self, Self) {
        let pipe = Arc::new(Pipe::new());
        pipe.acquire_read();
        pipe.acquire_write();
        (
            Self {
                pipe: pipe.clone(),
                kind: PipeEndpointKind::Read,
                nonblocking: Cell::new(nonblocking),
                direct: Cell::new(false),
                closed: Cell::new(false),
            },
            Self {
                pipe,
                kind: PipeEndpointKind::Write,
                nonblocking: Cell::new(nonblocking),
                direct: Cell::new(false),
                closed: Cell::new(false),
            },
        )
    }

    fn kind(&self) -> PipeEndpointKind {
        self.kind
    }

    fn nonblocking(&self) -> bool {
        self.nonblocking.get()
    }

// 本方法代码由AI完成
    fn read(&self, out: &mut [u8]) -> PipeResult<usize> {
        if self.kind != PipeEndpointKind::Read {
            return Err(PipeError::BrokenPipe);
        }
        if self.nonblocking.get() {
            self.pipe.try_read(out)
        } else {
            self.pipe.read(out)
        }
    }

// 本方法代码由AI完成
    fn write(&self, input: &[u8]) -> PipeResult<usize> {
        if self.kind != PipeEndpointKind::Write {
            return Err(PipeError::BrokenPipe);
        }
        if self.nonblocking.get() {
            self.pipe.try_write(input)
        } else {
            self.pipe.write(input)
        }
    }

// 本方法代码由AI完成
    fn close(&self) {
        self.release_once();
    }

// 本方法代码由AI完成
    fn poll_revents(&self, events: i16) -> PipeResult<i16> {
// 本变量代码由AI完成
        const POLLIN: i16 = 0x001;
// 本变量代码由AI完成
        const POLLOUT: i16 = 0x004;
        let raw = match self.kind {
            PipeEndpointKind::Read => self.pipe.poll_revents_read(),
            PipeEndpointKind::Write => self.pipe.poll_revents_write(),
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

// 本方法代码由AI完成
    fn poll_wait_for_ticks(
        &self,
        events: i16,
        timeout_ticks: u64,
        still_waiting: &mut dyn FnMut() -> bool,
    ) -> PipeResult<()> {
// 本变量代码由AI完成
        const POLLIN: i16 = 0x001;
// 本变量代码由AI完成
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
