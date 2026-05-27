//! 控制台与 pipe 的 [`VfsIoHandle`] 实现。

extern crate alloc;

use alloc::boxed::Box;

use api_v0::{VfsError, VfsIoHandle, VfsMetadata, VfsNodeType, VfsResult};
use ipc::pipe::{PipeEndpoint, PipeError};

fn console_chr_meta() -> VfsMetadata {
    VfsMetadata { node_type: VfsNodeType::Special,
                  size: 0,
                  mode: 0o20666 }
}

/// 标准输入占位：`read` 暂不支持（与迁移前 syscall 对 stdin 的 `EBADF` 一致）。
#[derive(Debug, Clone, Copy, Default)]
pub struct ConsoleInHandle;

impl VfsIoHandle for ConsoleInHandle {
    fn read(&mut self, _buf: &mut [u8]) -> VfsResult<usize> {
        Err(VfsError::BadFd)
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(console_chr_meta())
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(*self))
    }
}

/// 标准输出/错误：写入走控制台驱动。
#[derive(Debug, Clone, Copy, Default)]
pub struct ConsoleOutHandle;

impl VfsIoHandle for ConsoleOutHandle {
    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        console::write_raw_bytes(buf);
        Ok(buf.len())
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(console_chr_meta())
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(*self))
    }
}

/// pipe 读端。
pub struct PipeReadHandle(pub PipeEndpoint);

impl VfsIoHandle for PipeReadHandle {
    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        self.0.read(buf).map_err(map_pipe_err)
    }

    fn close(&mut self) -> VfsResult<()> {
        self.0.close();
        Ok(())
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self(self.0.clone())))
    }
}

/// pipe 写端。
pub struct PipeWriteHandle(pub PipeEndpoint);

impl VfsIoHandle for PipeWriteHandle {
    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        self.0.write(buf).map_err(map_pipe_err)
    }

    fn close(&mut self) -> VfsResult<()> {
        self.0.close();
        Ok(())
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self(self.0.clone())))
    }
}

fn map_pipe_err(err: PipeError) -> VfsError {
    match err {
        PipeError::WouldBlock => VfsError::WouldBlock,
        PipeError::BrokenPipe => VfsError::BrokenPipe,
        PipeError::InvalidCapacity => VfsError::Unsupported,
    }
}
