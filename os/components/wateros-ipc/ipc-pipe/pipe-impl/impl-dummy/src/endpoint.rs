//! fd 表 pipe 端点实现。

extern crate alloc;

use alloc::sync::Arc;
use api_v0::{PipeEndpointKind, PipeEndpointOps, PipeError, PipeResult};

use crate::kernel_pipe::Pipe;

/// 可放入 fd table 的 pipe 端点。
#[derive(Clone)]
pub struct PipeEndpoint {
    pipe: Arc<Pipe>,
    kind: PipeEndpointKind,
    nonblocking: bool,
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
            PipeEndpointKind::Read => self.pipe.close_read(),
            PipeEndpointKind::Write => self.pipe.close_write(),
        }
    }
}
