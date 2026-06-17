//! fd 表 pipe 端点实现。

extern crate alloc;

use alloc::sync::Arc;
use api_v0::{PipeEndpointKind, PipeEndpointOps, PipeError, PipeResult};
use waitqueue::TaskWaitResult;

use crate::kernel_pipe::Pipe;

/// 可放入 fd table 的 pipe 端点。
pub struct PipeEndpoint {
    pipe: Arc<Pipe>,
    kind: PipeEndpointKind,
    nonblocking: bool,
}

impl Clone for PipeEndpoint {
    fn clone(&self) -> Self {
        match self.kind {
            PipeEndpointKind::Read => self.pipe.acquire_read(),
            PipeEndpointKind::Write => self.pipe.acquire_write(),
        }
        Self {
            pipe: self.pipe.clone(),
            kind: self.kind,
            nonblocking: self.nonblocking,
        }
    }
}

impl PipeEndpoint {
    /// 创建一对读/写端点。
    pub fn pair(nonblocking: bool) -> (Self, Self) {
        PipeEndpointOps::pair(nonblocking)
    }

    /// 端点方向。
    #[inline]
    pub const fn kind(&self) -> PipeEndpointKind {
        self.kind
    }

    /// 是否按非阻塞语义执行。
    #[inline]
    pub const fn nonblocking(&self) -> bool {
        self.nonblocking
    }

    /// 从读端读取。
    pub fn read(&self, out: &mut [u8]) -> PipeResult<usize> {
        PipeEndpointOps::read(self, out)
    }

    /// 从写端写入。
    pub fn write(&self, input: &[u8]) -> PipeResult<usize> {
        PipeEndpointOps::write(self, input)
    }

    /// 显式关闭该端点。
    pub fn close(&self) {
        PipeEndpointOps::close(self);
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
                nonblocking,
            },
            Self {
                pipe,
                kind: PipeEndpointKind::Write,
                nonblocking,
            },
        )
    }

    fn kind(&self) -> PipeEndpointKind {
        self.kind
    }

    fn nonblocking(&self) -> bool {
        self.nonblocking
    }

    fn read(&self, out: &mut [u8]) -> PipeResult<usize> {
        if self.kind != PipeEndpointKind::Read {
            return Err(PipeError::BrokenPipe);
        }
        if self.nonblocking {
            self.pipe.try_read(out)
        } else {
            self.pipe.read(out)
        }
    }

    fn write(&self, input: &[u8]) -> PipeResult<usize> {
        if self.kind != PipeEndpointKind::Write {
            return Err(PipeError::BrokenPipe);
        }
        if self.nonblocking {
            self.pipe.try_write(input)
        } else {
            self.pipe.write(input)
        }
    }

    fn close(&self) {
        match self.kind {
            PipeEndpointKind::Read => self.pipe.release_read(),
            PipeEndpointKind::Write => self.pipe.release_write(),
        }
    }

    fn poll_revents(&self, events: i16) -> PipeResult<i16> {
        const POLLIN: i16 = 0x001;
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

    fn poll_wait_for_ticks(
        &self,
        events: i16,
        timeout_ticks: u64,
        still_waiting: &mut dyn FnMut() -> bool,
    ) -> PipeResult<()> {
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
