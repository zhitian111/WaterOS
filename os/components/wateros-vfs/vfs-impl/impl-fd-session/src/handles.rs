//! 控制台与 pipe 的 [`VfsIoHandle`] 实现。

extern crate alloc;

use alloc::boxed::Box;

use api_v0::{
    VfsError, VfsIoHandle, VfsMetadata, VfsNodeType, VfsResult, VfsSeekWhence,
};
use ipc::pipe::{PipeEndpoint, PipeEndpointOps, PipeError};

fn console_chr_meta() -> VfsMetadata {
    VfsMetadata { node_type: VfsNodeType::Special,
                  size: 0,
                  mode: 0o20666 }
}

/// 标准输入占位：bring-up 无真实输入源时 `read` 返回 EOF。
#[derive(Debug, Clone, Copy, Default)]
pub struct ConsoleInHandle;

impl VfsIoHandle for ConsoleInHandle {
    fn read(&mut self, _buf: &mut [u8]) -> VfsResult<usize> {
        Ok(0)
    }

    fn seek(&mut self, _offset: i64, _whence: VfsSeekWhence) -> VfsResult<u64> {
        Err(VfsError::Unsupported)
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

    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
        const POLLOUT: i16 = 0x004;
        if events & POLLOUT != 0 {
            Ok(POLLOUT)
        } else {
            Ok(0)
        }
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

    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
        self.0.poll_revents(events).map_err(map_pipe_err)
    }

    fn poll_wait_for_ticks(
        &mut self,
        events: i16,
        timeout_ticks: u64,
        still_waiting: &mut dyn FnMut() -> bool,
    ) -> VfsResult<()> {
        self.0
            .poll_wait_for_ticks(events, timeout_ticks, still_waiting)
            .map_err(map_pipe_err)
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

    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
        self.0.poll_revents(events).map_err(map_pipe_err)
    }

    fn poll_wait_for_ticks(
        &mut self,
        events: i16,
        timeout_ticks: u64,
        still_waiting: &mut dyn FnMut() -> bool,
    ) -> VfsResult<()> {
        self.0
            .poll_wait_for_ticks(events, timeout_ticks, still_waiting)
            .map_err(map_pipe_err)
    }

    fn close(&mut self) -> VfsResult<()> {
        self.0.close();
        Ok(())
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self(self.0.clone())))
    }
}

/// bring-up：空 pipe 读端无 `POLLIN`，写入后应就绪（供 `ppoll` 路径使用）。
pub fn poll_pipe_smoke() -> bool {
    const POLLIN: i16 = 0x001;
    let (read_ep, write_ep) = PipeEndpoint::pair(false);
    let mut read = PipeReadHandle(read_ep);
    let mut write = PipeWriteHandle(write_ep);
    if read.poll_revents(POLLIN).ok() != Some(0) {
        return false;
    }
    if write.write(b"x").is_err() {
        return false;
    }
    read.poll_revents(POLLIN).ok() == Some(POLLIN)
}

fn map_pipe_err(err: PipeError) -> VfsError {
    match err {
        PipeError::WouldBlock => VfsError::WouldBlock,
        PipeError::BrokenPipe => VfsError::BrokenPipe,
        PipeError::InvalidCapacity => VfsError::Unsupported,
    }
}
