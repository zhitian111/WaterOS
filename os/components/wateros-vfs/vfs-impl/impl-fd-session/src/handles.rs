//! 控制台与 pipe 的 [`VfsIoHandle`] 实现。

extern crate alloc;

use alloc::boxed::Box;
use core::sync::atomic::{AtomicU64, Ordering};

use api_v0::{
    VfsError, VfsIoHandle, VfsMetadata, VfsNodeType, VfsResult, VfsSeekWhence,
};
use ipc::pipe::{PipeEndpoint, PipeEndpointOps, PipeError};

static NEXT_PIPE_INODE: AtomicU64 = AtomicU64::new(1);

fn special_meta(mode: u16, inode: u64) -> VfsMetadata {
    VfsMetadata {
        node_type: VfsNodeType::Special,
        size: 0,
        mode,
        device_major: 0,
        device_minor: 0x7fff_0001,
        inode,
        mount_id: 0,
        nlink: 1,
    }
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
        Ok(special_meta(0o20666, 1))
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
        Ok(special_meta(0o20666, 1))
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(*self))
    }
}

/// `/dev/null`：读 EOF，写入丢弃。
#[derive(Debug, Clone, Copy, Default)]
pub struct NullDeviceHandle;

impl VfsIoHandle for NullDeviceHandle {
    fn read(&mut self, _buf: &mut [u8]) -> VfsResult<usize> {
        Ok(0)
    }

    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        Ok(buf.len())
    }

    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
        const POLLIN: i16 = 0x001;
        const POLLOUT: i16 = 0x004;
        Ok(events & (POLLIN | POLLOUT))
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(special_meta(0o20666, 2))
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(*self))
    }
}

/// `/dev/zero`：读出零字节，写入丢弃。
#[derive(Debug, Clone, Copy, Default)]
pub struct ZeroDeviceHandle;

impl VfsIoHandle for ZeroDeviceHandle {
    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        buf.fill(0);
        Ok(buf.len())
    }

    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        Ok(buf.len())
    }

    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
        const POLLIN: i16 = 0x001;
        const POLLOUT: i16 = 0x004;
        Ok(events & (POLLIN | POLLOUT))
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(special_meta(0o20666, 3))
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(*self))
    }
}

/// pipe 读端。
pub struct PipeReadHandle {
    endpoint: PipeEndpoint,
    inode: u64,
}

pub struct PipeWriteHandle {
    endpoint: PipeEndpoint,
    inode: u64,
}

pub fn pipe_handle_pair(nonblocking: bool) -> (PipeReadHandle, PipeWriteHandle) {
    let (read, write) = PipeEndpoint::pair(nonblocking);
    let inode = NEXT_PIPE_INODE.fetch_add(1, Ordering::Relaxed);
    (
        PipeReadHandle { endpoint: read, inode },
        PipeWriteHandle { endpoint: write, inode },
    )
}

impl VfsIoHandle for PipeReadHandle {
    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        self.endpoint.read(buf).map_err(map_pipe_err)
    }

    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
        self.endpoint.poll_revents(events).map_err(map_pipe_err)
    }

    fn poll_wait_for_ticks(
        &mut self,
        events: i16,
        timeout_ticks: u64,
        still_waiting: &mut dyn FnMut() -> bool,
    ) -> VfsResult<()> {
        self.endpoint
            .poll_wait_for_ticks(events, timeout_ticks, still_waiting)
            .map_err(map_pipe_err)
    }

    fn close(&mut self) -> VfsResult<()> {
        self.endpoint.close();
        Ok(())
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(special_meta(0o10600, self.inode))
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self {
            endpoint: self.endpoint.clone(),
            inode: self.inode,
        }))
    }
}

/// pipe 写端。
impl VfsIoHandle for PipeWriteHandle {
    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        self.endpoint.write(buf).map_err(map_pipe_err)
    }

    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
        self.endpoint.poll_revents(events).map_err(map_pipe_err)
    }

    fn poll_wait_for_ticks(
        &mut self,
        events: i16,
        timeout_ticks: u64,
        still_waiting: &mut dyn FnMut() -> bool,
    ) -> VfsResult<()> {
        self.endpoint
            .poll_wait_for_ticks(events, timeout_ticks, still_waiting)
            .map_err(map_pipe_err)
    }

    fn close(&mut self) -> VfsResult<()> {
        self.endpoint.close();
        Ok(())
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(special_meta(0o10600, self.inode))
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self {
            endpoint: self.endpoint.clone(),
            inode: self.inode,
        }))
    }
}

/// bring-up：空 pipe 读端无 `POLLIN`，写入后应就绪（供 `ppoll` 路径使用）。
pub fn poll_pipe_smoke() -> bool {
    const POLLIN: i16 = 0x001;
    let (mut read, mut write) = pipe_handle_pair(false);
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
        PipeError::Interrupted => VfsError::Interrupted,
        PipeError::BrokenPipe => VfsError::BrokenPipe,
        PipeError::InvalidCapacity => VfsError::Unsupported,
    }
}
