//! 控制台与 pipe 的 [`VfsIoHandle`] 实现。

extern crate alloc;

use alloc::boxed::Box;
use core::sync::atomic::{AtomicU64, Ordering};

use api_v0::{
    VfsError, VfsIoHandle, VfsMetadata, VfsNodeType, VfsResult, VfsSeekWhence,
};
use ipc::pipe::{PipeEndpoint, PipeEndpointOps, PipeError};

static NEXT_PIPE_INODE: AtomicU64 = AtomicU64::new(1);
static NEXT_STREAM_PAIR_INODE: AtomicU64 = AtomicU64::new(1);
static URANDOM_STATE: AtomicU64 = AtomicU64::new(0x6a09_e667_f3bc_c909);

fn special_meta(mode: u16, inode: u64) -> VfsMetadata {
    special_dev_meta(mode, inode, 0, 0x7fff_0001)
}

fn special_dev_meta(mode: u16, inode: u64, device_major: u32, device_minor: u32) -> VfsMetadata {
    VfsMetadata {
        node_type: VfsNodeType::Special,
        size: 0,
        mode,
        device_major,
        device_minor,
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
        Ok(special_dev_meta(0o20666, 2, 1, 3))
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
        Ok(special_dev_meta(0o20666, 3, 1, 5))
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(*self))
    }
}

/// `/dev/cpu_dma_latency`：cyclictest 写入 latency 值；stub 吞掉写入即可。
#[derive(Debug, Clone, Copy, Default)]
pub struct CpuDmaLatencyDeviceHandle;

impl VfsIoHandle for CpuDmaLatencyDeviceHandle {
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

    fn ioctl(&mut self, _request: usize, _arg: usize) -> VfsResult<isize> {
        Err(VfsError::Unsupported)
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(special_dev_meta(0o20600, 5, 10, 233))
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(*self))
    }
}

/// `/dev/urandom`：早期兼容伪随机字节流，满足 libc/benchmark 对随机设备的读取需求。
#[derive(Debug, Clone, Copy, Default)]
pub struct UrandomDeviceHandle;

impl VfsIoHandle for UrandomDeviceHandle {
    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        let mut state = URANDOM_STATE.fetch_add(
            0x9e37_79b9_7f4a_7c15u64 ^ (buf.as_ptr() as u64).rotate_left(17),
            Ordering::Relaxed,
        );
        for byte in buf.iter_mut() {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = (state >> 24) as u8;
        }
        URANDOM_STATE.store(state, Ordering::Relaxed);
        Ok(buf.len())
    }

    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        let mut mix = buf.len() as u64;
        for byte in buf.iter().take(32) {
            mix = mix.rotate_left(5) ^ (*byte as u64);
        }
        URANDOM_STATE.fetch_xor(mix, Ordering::Relaxed);
        Ok(buf.len())
    }

    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
        const POLLIN: i16 = 0x001;
        const POLLOUT: i16 = 0x004;
        Ok(events & (POLLIN | POLLOUT))
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(special_dev_meta(0o20666, 4, 1, 9))
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

    fn open_status_flags(&self) -> u32 {
        if self.endpoint.nonblocking() {
            0o0004000
        } else {
            0
        }
    }

    fn set_open_status_flags(&mut self, flags: u32) -> VfsResult<()> {
        self.endpoint.set_nonblocking(flags & 0o0004000 != 0);
        Ok(())
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

    fn open_status_flags(&self) -> u32 {
        if self.endpoint.nonblocking() {
            0o0004000
        } else {
            0
        }
    }

    fn set_open_status_flags(&mut self, flags: u32) -> VfsResult<()> {
        self.endpoint.set_nonblocking(flags & 0o0004000 != 0);
        Ok(())
    }
}

/// Unix domain stream socket pair 的一端：读/写分别连到交叉 pipe。
pub struct UnixStreamPairEnd {
    read_end: PipeEndpoint,
    write_end: PipeEndpoint,
    inode: u64,
}

pub fn stream_pair_handle_pair(nonblocking: bool) -> (UnixStreamPairEnd, UnixStreamPairEnd) {
    let (read_ab, write_ab) = PipeEndpoint::pair(nonblocking);
    let (read_ba, write_ba) = PipeEndpoint::pair(nonblocking);
    let inode = NEXT_STREAM_PAIR_INODE.fetch_add(1, Ordering::Relaxed);
    (
        UnixStreamPairEnd {
            read_end: read_ba,
            write_end: write_ab,
            inode,
        },
        UnixStreamPairEnd {
            read_end: read_ab,
            write_end: write_ba,
            inode,
        },
    )
}

impl VfsIoHandle for UnixStreamPairEnd {
    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        self.read_end.read(buf).map_err(map_pipe_err)
    }

    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        self.write_end.write(buf).map_err(map_pipe_err)
    }

    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
        const POLLIN: i16 = 0x001;
        const POLLOUT: i16 = 0x004;
        let mut revents = 0i16;
        if events & POLLIN != 0 {
            revents |= self.read_end.poll_revents(POLLIN).map_err(map_pipe_err)?;
        }
        if events & POLLOUT != 0 {
            revents |= self.write_end.poll_revents(POLLOUT).map_err(map_pipe_err)?;
        }
        Ok(revents)
    }

    fn poll_wait_for_ticks(
        &mut self,
        events: i16,
        timeout_ticks: u64,
        still_waiting: &mut dyn FnMut() -> bool,
    ) -> VfsResult<()> {
        const POLLIN: i16 = 0x001;
        const POLLOUT: i16 = 0x004;
        if events & POLLIN != 0 {
            self.read_end
                .poll_wait_for_ticks(POLLIN, timeout_ticks, still_waiting)
                .map_err(map_pipe_err)?;
        }
        if events & POLLOUT != 0 && still_waiting() {
            self.write_end
                .poll_wait_for_ticks(POLLOUT, timeout_ticks, still_waiting)
                .map_err(map_pipe_err)?;
        }
        Ok(())
    }

    fn close(&mut self) -> VfsResult<()> {
        self.read_end.close();
        self.write_end.close();
        Ok(())
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(special_meta(0o140600, self.inode))
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self {
            read_end: self.read_end.clone(),
            write_end: self.write_end.clone(),
            inode: self.inode,
        }))
    }

    fn open_status_flags(&self) -> u32 {
        if self.read_end.nonblocking() {
            0o0004000
        } else {
            0
        }
    }

    fn set_open_status_flags(&mut self, flags: u32) -> VfsResult<()> {
        let nonblocking = flags & 0o0004000 != 0;
        self.read_end.set_nonblocking(nonblocking);
        self.write_end.set_nonblocking(nonblocking);
        Ok(())
    }
}

/// bring-up：验证 socketpair 双向读写与 poll。
pub fn stream_pair_smoke() -> bool {
    const POLLIN: i16 = 0x001;
    let (mut a, mut b) = stream_pair_handle_pair(false);
    if a.write(b"ab").is_err() {
        return false;
    }
    let mut buf = [0u8; 2];
    if b.read(&mut buf).ok() != Some(2) || &buf != b"ab" {
        return false;
    }
    if b.write(b"xy").is_err() {
        return false;
    }
    if a.read(&mut buf).ok() != Some(2) || &buf != b"xy" {
        return false;
    }
    if a.poll_revents(POLLIN).ok() != Some(0) {
        return false;
    }
    true
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
