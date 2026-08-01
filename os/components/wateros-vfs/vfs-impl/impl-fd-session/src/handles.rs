//! 控制台与 pipe 的 [`VfsIoHandle`] 实现。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use api_v0::{
    VfsCopyProgress, VfsError, VfsIoHandle, VfsMetadata, VfsNodeType, VfsPreparedRead,
    VfsReadFinish, VfsReadLease, VfsResult, VfsSeekWhence,
};
use ipc::pipe::{
    NamedPipe, PipeEndpoint, PipeEndpointOps, PipeError, PipeReadFinish as IpcPipeReadFinish,
    PipeReadLease as IpcPipeReadLease,
};
use spin::Mutex;

// 本变量代码由AI完成
static NEXT_PIPE_INODE: AtomicU64 = AtomicU64::new(1);
static NAMED_PIPES: Mutex<BTreeMap<(u64, u64), Weak<NamedPipe>>> = Mutex::new(BTreeMap::new());
// 本变量代码由AI完成
static NEXT_STREAM_PAIR_INODE: AtomicU64 = AtomicU64::new(1);
// 本变量代码由AI完成
struct UrandomState {
    value: u64,
    active_read: Option<u64>,
    next_read_id: u64,
    pending_mix: u64,
}

static URANDOM_STATE: Mutex<UrandomState> = Mutex::new(UrandomState {
    value: 0x6A09_E667_F3BC_C909,
    active_read: None,
    next_read_id: 1,
    pending_mix: 0,
});
const O_NONBLOCK: u32 = 0o0004000;
const O_DIRECT: u32 = 0o00040000;

#[derive(Clone, Copy)]
enum GeneratedReadKind {
    Empty,
    Zero,
}

struct GeneratedPreparedRead {
    kind: GeneratedReadKind,
    max_len: usize,
}

impl VfsPreparedRead for GeneratedPreparedRead {
    fn acquire(self: Box<Self>) -> VfsResult<Box<dyn VfsReadLease>> {
        let mut data = Vec::new();
        if matches!(self.kind, GeneratedReadKind::Zero) {
            data.try_reserve_exact(self.max_len)
                .map_err(|_| VfsError::NoMemory)?;
            data.resize(self.max_len, 0);
        }
        Ok(Box::new(GeneratedReadLease { data }))
    }
}

struct GeneratedReadLease {
    data: Vec<u8>,
}

impl VfsReadLease for GeneratedReadLease {
    fn bytes(&self) -> &[u8] {
        self.data.as_slice()
    }

    fn finish(self: Box<Self>, progress: VfsCopyProgress) -> VfsResult<VfsReadFinish> {
        if progress.copied > self.data.len() {
            return Err(VfsError::Io);
        }
        if progress.copied == 0 && !progress.complete {
            Ok(VfsReadFinish::Fault)
        } else {
            Ok(VfsReadFinish::Bytes(progress.copied))
        }
    }
}

fn generated_read(kind: GeneratedReadKind, max_len: usize) -> Box<dyn VfsPreparedRead> {
    Box::new(GeneratedPreparedRead { kind, max_len })
}

fn urandom_next(state: &mut u64) -> u8 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state >> 24) as u8
}

fn urandom_advance(mut state: u64, len: usize) -> u64 {
    for _ in 0..len {
        let _ = urandom_next(&mut state);
    }
    state
}

fn urandom_mix(buf: &[u8]) -> u64 {
    let mut mix = buf.len() as u64;
    for byte in buf.iter().take(32) {
        mix = mix.rotate_left(5) ^ (*byte as u64);
    }
    mix
}

struct UrandomPreparedRead {
    max_len: usize,
}

impl VfsPreparedRead for UrandomPreparedRead {
    fn acquire(self: Box<Self>) -> VfsResult<Box<dyn VfsReadLease>> {
        let (id, start) = {
            let mut state = URANDOM_STATE.lock();
            if state.active_read.is_some() {
                return Err(VfsError::Busy);
            }
            let id = state.next_read_id;
            state.next_read_id = state.next_read_id.wrapping_add(1);
            state.active_read = Some(id);
            state.pending_mix = 0;
            (id, state.value)
        };
        let mut data = Vec::new();
        if data.try_reserve_exact(self.max_len).is_err() {
            cancel_urandom_read(id, start);
            return Err(VfsError::NoMemory);
        }
        let mut generated_state = start;
        for _ in 0..self.max_len {
            data.push(urandom_next(&mut generated_state));
        }
        Ok(Box::new(UrandomReadLease {
            id,
            start,
            data,
            active: true,
        }))
    }
}

fn cancel_urandom_read(id: u64, start: u64) {
    let mut state = URANDOM_STATE.lock();
    if state.active_read == Some(id) {
        state.value = start ^ state.pending_mix;
        state.pending_mix = 0;
        state.active_read = None;
    }
}

struct UrandomReadLease {
    id: u64,
    start: u64,
    data: Vec<u8>,
    active: bool,
}

impl VfsReadLease for UrandomReadLease {
    fn bytes(&self) -> &[u8] {
        self.data.as_slice()
    }

    fn finish(mut self: Box<Self>, progress: VfsCopyProgress) -> VfsResult<VfsReadFinish> {
        if progress.copied > self.data.len() {
            return Err(VfsError::Io);
        }
        let committed_state = urandom_advance(self.start, progress.copied);
        {
            let mut state = URANDOM_STATE.lock();
            if state.active_read != Some(self.id) {
                return Err(VfsError::Io);
            }
            state.value = committed_state ^ state.pending_mix;
            state.pending_mix = 0;
            state.active_read = None;
        }
        self.active = false;
        if progress.copied == 0 && !progress.complete {
            Ok(VfsReadFinish::Fault)
        } else {
            Ok(VfsReadFinish::Bytes(progress.copied))
        }
    }
}

impl Drop for UrandomReadLease {
    fn drop(&mut self) {
        if self.active {
            cancel_urandom_read(self.id, self.start);
        }
    }
}

// 本方法代码由AI完成
fn special_meta(mode: u16, inode: u64) -> VfsMetadata {
    special_dev_meta(mode, inode, 0, 0x7fff_0001)
}

// 本方法代码由AI完成
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
        uid: 0,
        gid: 0,
    }
}

/// 标准输入占位：bring-up 无真实输入源时 `read` 返回 EOF。
#[derive(Debug, Clone, Copy, Default)]
// 本结构代码由AI完成
pub struct ConsoleInHandle;

impl VfsIoHandle for ConsoleInHandle {
    fn open_accmode(&self) -> u32 {
        0
    }

    fn prepare_read(&mut self, max_len: usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        Ok(generated_read(GeneratedReadKind::Empty, max_len))
    }

// 本方法代码由AI完成
    fn read(&mut self, _buf: &mut [u8]) -> VfsResult<usize> {
        Ok(0)
    }

// 本方法代码由AI完成
    fn seek(&mut self, _offset: i64, _whence: VfsSeekWhence) -> VfsResult<u64> {
        Err(VfsError::Unsupported)
    }

// 本方法代码由AI完成
    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(special_meta(0o20666, 1))
    }

// 本方法代码由AI完成
    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(*self))
    }

    fn is_tty_char_device(&self) -> bool {
        true
    }
}

/// 标准输出/错误：写入走控制台驱动。
#[derive(Debug, Clone, Copy, Default)]
// 本结构代码由AI完成
pub struct ConsoleOutHandle;

impl VfsIoHandle for ConsoleOutHandle {
    fn open_accmode(&self) -> u32 {
        1
    }

// 本方法代码由AI完成
    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        console::write_raw_bytes(buf);
        Ok(buf.len())
    }

// 本方法代码由AI完成
    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
// 本变量代码由AI完成
        const POLLOUT: i16 = 0x004;
        if events & POLLOUT != 0 {
            Ok(POLLOUT)
        } else {
            Ok(0)
        }
    }

// 本方法代码由AI完成
    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(special_meta(0o20666, 1))
    }

// 本方法代码由AI完成
    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(*self))
    }

    fn is_tty_char_device(&self) -> bool {
        true
    }
}

/// `/dev/null`：读 EOF，写入丢弃。
#[derive(Debug, Clone, Copy)]
// 本结构代码由AI完成
pub struct NullDeviceHandle {
    accmode: u32,
}

impl NullDeviceHandle {
    pub const fn new(accmode: u32) -> Self {
        Self { accmode }
    }
}

impl Default for NullDeviceHandle {
    fn default() -> Self {
        Self::new(2)
    }
}

impl VfsIoHandle for NullDeviceHandle {
    fn open_accmode(&self) -> u32 {
        self.accmode
    }

    fn prepare_read(&mut self, max_len: usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        Ok(generated_read(GeneratedReadKind::Empty, max_len))
    }

// 本方法代码由AI完成
    fn read(&mut self, _buf: &mut [u8]) -> VfsResult<usize> {
        Ok(0)
    }

// 本方法代码由AI完成
    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        Ok(buf.len())
    }

// 本方法代码由AI完成
    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
// 本变量代码由AI完成
        const POLLIN: i16 = 0x001;
// 本变量代码由AI完成
        const POLLOUT: i16 = 0x004;
        Ok(events & (POLLIN | POLLOUT))
    }

// 本方法代码由AI完成
    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(special_dev_meta(0o20666, 2, 1, 3))
    }

// 本方法代码由AI完成
    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(*self))
    }
}

/// `/dev/zero`：读出零字节，写入丢弃。
#[derive(Debug, Clone, Copy)]
// 本结构代码由AI完成
pub struct ZeroDeviceHandle {
    accmode: u32,
}

impl ZeroDeviceHandle {
    pub const fn new(accmode: u32) -> Self {
        Self { accmode }
    }
}

impl Default for ZeroDeviceHandle {
    fn default() -> Self {
        Self::new(2)
    }
}

impl VfsIoHandle for ZeroDeviceHandle {
    fn open_accmode(&self) -> u32 {
        self.accmode
    }

    fn prepare_read(&mut self, max_len: usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        Ok(generated_read(GeneratedReadKind::Zero, max_len))
    }

// 本方法代码由AI完成
    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        buf.fill(0);
        Ok(buf.len())
    }

// 本方法代码由AI完成
    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        Ok(buf.len())
    }

// 本方法代码由AI完成
    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
// 本变量代码由AI完成
        const POLLIN: i16 = 0x001;
// 本变量代码由AI完成
        const POLLOUT: i16 = 0x004;
        Ok(events & (POLLIN | POLLOUT))
    }

// 本方法代码由AI完成
    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(special_dev_meta(0o20666, 3, 1, 5))
    }

// 本方法代码由AI完成
    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(*self))
    }
}

/// `/dev/cpu_dma_latency`：cyclictest 写入 latency 值；stub 吞掉写入即可。
#[derive(Debug, Clone, Copy)]
// 本结构代码由AI完成
pub struct CpuDmaLatencyDeviceHandle {
    accmode: u32,
}

impl CpuDmaLatencyDeviceHandle {
    pub const fn new(accmode: u32) -> Self {
        Self { accmode }
    }
}

impl Default for CpuDmaLatencyDeviceHandle {
    fn default() -> Self {
        Self::new(2)
    }
}

impl VfsIoHandle for CpuDmaLatencyDeviceHandle {
    fn open_accmode(&self) -> u32 {
        self.accmode
    }

    fn prepare_read(&mut self, max_len: usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        Ok(generated_read(GeneratedReadKind::Empty, max_len))
    }

// 本方法代码由AI完成
    fn read(&mut self, _buf: &mut [u8]) -> VfsResult<usize> {
        Ok(0)
    }

// 本方法代码由AI完成
    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        Ok(buf.len())
    }

// 本方法代码由AI完成
    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
// 本变量代码由AI完成
        const POLLIN: i16 = 0x001;
// 本变量代码由AI完成
        const POLLOUT: i16 = 0x004;
        Ok(events & (POLLIN | POLLOUT))
    }

// 本方法代码由AI完成
    fn ioctl(&mut self, _request: usize, _arg: usize) -> VfsResult<isize> {
        Err(VfsError::Unsupported)
    }

// 本方法代码由AI完成
    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(special_dev_meta(0o20600, 5, 10, 233))
    }

// 本方法代码由AI完成
    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(*self))
    }
}

/// `/dev/urandom`：早期兼容伪随机字节流，满足 libc/benchmark 对随机设备的读取需求。
#[derive(Debug, Clone, Copy)]
// 本结构代码由AI完成
pub struct UrandomDeviceHandle {
    accmode: u32,
}

impl UrandomDeviceHandle {
    pub const fn new(accmode: u32) -> Self { Self { accmode } }
}

impl Default for UrandomDeviceHandle {
    fn default() -> Self { Self::new(2) }
}

impl VfsIoHandle for UrandomDeviceHandle {
    fn open_accmode(&self) -> u32 { self.accmode }

    fn prepare_read(&mut self, max_len: usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        Ok(Box::new(UrandomPreparedRead { max_len }))
    }

// 本方法代码由AI完成
    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        let prepared = self.prepare_read(buf.len())?;
        let lease = prepared.acquire()?;
        let len = lease.bytes().len();
        buf[..len].copy_from_slice(lease.bytes());
        match lease.finish(VfsCopyProgress { copied: len, complete: true })? {
            VfsReadFinish::Bytes(copied) => Ok(copied),
            VfsReadFinish::Fault => Err(VfsError::Io),
        }
    }

// 本方法代码由AI完成
    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        let mix = urandom_mix(buf);
        let mut state = URANDOM_STATE.lock();
        if state.active_read.is_some() {
            state.pending_mix ^= mix;
        } else {
            state.value ^= mix;
        }
        Ok(buf.len())
    }

// 本方法代码由AI完成
    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
// 本变量代码由AI完成
        const POLLIN: i16 = 0x001;
// 本变量代码由AI完成
        const POLLOUT: i16 = 0x004;
        Ok(events & (POLLIN | POLLOUT))
    }

// 本方法代码由AI完成
    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(special_dev_meta(0o20666, 4, 1, 9))
    }

// 本方法代码由AI完成
    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(*self))
    }
}

pub(crate) fn read_lease_self_test() {
    let mut zero = ZeroDeviceHandle::default();
    let lease = zero
        .prepare_read(3)
        .expect("prepare zero read")
        .acquire()
        .expect("acquire zero read");
    assert_eq!(lease.bytes(), &[0, 0, 0]);
    assert_eq!(
        lease.finish(VfsCopyProgress {
            copied: 1,
            complete: false
        }),
        Ok(VfsReadFinish::Bytes(1))
    );

    const START: u64 = 0x6A09_E667_F3BC_C909;
    {
        let mut state = URANDOM_STATE.lock();
        state.value = START;
        state.active_read = None;
        state.pending_mix = 0;
    }
    let mut random = UrandomDeviceHandle::default();
    let cancelled = random
        .prepare_read(4)
        .expect("prepare random read")
        .acquire()
        .expect("acquire random read");
    let first = cancelled.bytes().to_vec();
    assert_eq!(
        cancelled.finish(VfsCopyProgress {
            copied: 0,
            complete: false
        }),
        Ok(VfsReadFinish::Fault)
    );
    let partial = random
        .prepare_read(4)
        .expect("prepare random retry")
        .acquire()
        .expect("acquire random retry");
    assert_eq!(partial.bytes(), first.as_slice());
    assert_eq!(
        partial.finish(VfsCopyProgress {
            copied: 2,
            complete: false
        }),
        Ok(VfsReadFinish::Bytes(2))
    );
    assert_eq!(URANDOM_STATE.lock().value, urandom_advance(START, 2));

    let concurrent = random
        .prepare_read(2)
        .expect("prepare random concurrent write")
        .acquire()
        .expect("acquire random concurrent write");
    let before_write = URANDOM_STATE.lock().value;
    let entropy = b"mix";
    assert_eq!(random.write(entropy), Ok(entropy.len()));
    assert_eq!(
        concurrent.finish(VfsCopyProgress {
            copied: 2,
            complete: true
        }),
        Ok(VfsReadFinish::Bytes(2))
    );
    assert_eq!(
        URANDOM_STATE.lock().value,
        urandom_advance(before_write, 2) ^ urandom_mix(entropy)
    );
    let mut state = URANDOM_STATE.lock();
    state.value = START;
    state.active_read = None;
    state.pending_mix = 0;
}

/// pipe 读端。
// 本结构代码由AI完成
pub struct PipeReadHandle {
    endpoint: PipeEndpoint,
    /// 合成 inode 号，供 `flock` / `stat` 区分 pipe 实例。
    inode: u64,
}

/// pipe 写端。
// 本结构代码由AI完成
pub struct PipeWriteHandle {
    endpoint: PipeEndpoint,
    inode: u64,
}

/// One open file description for a filesystem FIFO.
pub struct NamedPipeHandle {
    named: Arc<NamedPipe>,
    registry_key: (u64, u64),
    read_end: Option<PipeEndpoint>,
    write_end: Option<PipeEndpoint>,
    metadata: VfsMetadata,
}

pub fn open_named_pipe(
    metadata: VfsMetadata,
    flags: api_v0::VfsOpenFlags,
) -> VfsResult<Box<dyn VfsIoHandle>> {
    let key = (metadata.mount_id, metadata.inode);
    let named = {
        let mut registry = NAMED_PIPES.lock();
        if let Some(existing) = registry.get(&key).and_then(Weak::upgrade) {
            existing
        } else {
            let created = Arc::new(NamedPipe::new());
            registry.insert(key, Arc::downgrade(&created));
            created
        }
    };
    let wants_read = flags.contains(api_v0::VfsOpenFlags::READ);
    let wants_write = flags.contains(api_v0::VfsOpenFlags::WRITE);
    let nonblocking = flags.contains(api_v0::VfsOpenFlags::NONBLOCK);

    let (read_end, write_end) = if wants_read && wants_write {
        let read = named.open_read(true).map_err(map_pipe_err)?;
        let write = named.open_write(true).map_err(map_named_pipe_open_error)?;
        read.set_nonblocking(nonblocking);
        write.set_nonblocking(nonblocking);
        (Some(read), Some(write))
    } else if wants_read {
        (Some(named.open_read(nonblocking).map_err(map_pipe_err)?), None)
    } else if wants_write {
        (
            None,
            Some(
                named
                    .open_write(nonblocking)
                    .map_err(map_named_pipe_open_error)?,
            ),
        )
    } else {
        return Err(VfsError::InvalidPath);
    };

    Ok(Box::new(NamedPipeHandle {
        named,
        registry_key: key,
        read_end,
        write_end,
        metadata,
    }))
}

impl Drop for NamedPipeHandle {
    fn drop(&mut self) {
        if Arc::strong_count(&self.named) != 1 {
            return;
        }
        let mut registry = NAMED_PIPES.lock();
        let owns_entry = registry
            .get(&self.registry_key)
            .is_some_and(|weak| weak.as_ptr() == Arc::as_ptr(&self.named));
        if owns_entry {
            registry.remove(&self.registry_key);
        }
    }
}

fn map_named_pipe_open_error(error: PipeError) -> VfsError {
    if error == PipeError::BrokenPipe {
        VfsError::NoDevice
    } else {
        map_pipe_err(error)
    }
}

// 本方法代码由AI完成
pub fn pipe_handle_pair(nonblocking: bool) -> (PipeReadHandle, PipeWriteHandle) {
    pipe_handle_pair_with_flags(nonblocking, false)
}

// 本方法代码由AI完成
pub fn pipe_handle_pair_with_flags(
    nonblocking: bool,
    direct: bool,
) -> (PipeReadHandle, PipeWriteHandle) {
    let (read, write) = PipeEndpoint::pair_with_flags(nonblocking, direct);
    let inode = NEXT_PIPE_INODE.fetch_add(1, Ordering::Relaxed);
    (
        PipeReadHandle {
            endpoint: read,
            inode,
        },
        PipeWriteHandle {
            endpoint: write,
            inode,
        },
    )
}

impl VfsIoHandle for NamedPipeHandle {
    fn open_accmode(&self) -> u32 {
        match (self.read_end.is_some(), self.write_end.is_some()) {
            (true, true) => 2,
            (false, true) => 1,
            _ => 0,
        }
    }

    fn prepare_read(&mut self, max_len: usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        let endpoint = self.read_end.as_ref().ok_or(VfsError::BadFd)?;
        Ok(Box::new(PipePreparedRead {
            endpoint: endpoint.clone(),
            max_len,
        }))
    }

    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        self.read_end
            .as_ref()
            .ok_or(VfsError::BadFd)?
            .read(buf)
            .map_err(map_pipe_err)
    }

    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        self.write_end
            .as_ref()
            .ok_or(VfsError::BadFd)?
            .write(buf)
            .map_err(map_pipe_err)
    }

    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
        const POLLIN: i16 = 0x001;
        const POLLOUT: i16 = 0x004;
        let mut revents = 0;
        if events & POLLIN != 0 {
            if let Some(read) = &self.read_end {
                revents |= read.poll_revents(POLLIN).map_err(map_pipe_err)?;
            }
        }
        if events & POLLOUT != 0 {
            if let Some(write) = &self.write_end {
                revents |= write.poll_revents(POLLOUT).map_err(map_pipe_err)?;
            }
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
            if let Some(read) = &self.read_end {
                return read
                    .poll_wait_for_ticks(POLLIN, timeout_ticks, still_waiting)
                    .map_err(map_pipe_err);
            }
        }
        if events & POLLOUT != 0 {
            if let Some(write) = &self.write_end {
                return write
                    .poll_wait_for_ticks(POLLOUT, timeout_ticks, still_waiting)
                    .map_err(map_pipe_err);
            }
        }
        Ok(())
    }

    fn close(&mut self) -> VfsResult<()> {
        if let Some(read) = &self.read_end {
            read.close();
        }
        if let Some(write) = &self.write_end {
            write.close();
        }
        Ok(())
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(self.metadata.clone())
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self {
            named: self.named.clone(),
            registry_key: self.registry_key,
            read_end: self.read_end.clone(),
            write_end: self.write_end.clone(),
            metadata: self.metadata.clone(),
        }))
    }

    fn open_status_flags(&self) -> u32 {
        let endpoint = self.read_end.as_ref().or(self.write_end.as_ref());
        endpoint
            .filter(|endpoint| endpoint.nonblocking())
            .map_or(0, |_| O_NONBLOCK)
    }

    fn set_open_status_flags(&mut self, flags: u32) -> VfsResult<()> {
        let nonblocking = flags & O_NONBLOCK != 0;
        if let Some(read) = &self.read_end {
            read.set_nonblocking(nonblocking);
        }
        if let Some(write) = &self.write_end {
            write.set_nonblocking(nonblocking);
        }
        Ok(())
    }

    fn pipe_capacity(&self) -> Option<usize> {
        self.read_end
            .as_ref()
            .or(self.write_end.as_ref())
            .map(PipeEndpoint::pipe_capacity)
    }

    fn pipe_buffer_len(&self) -> Option<usize> {
        self.read_end
            .as_ref()
            .or(self.write_end.as_ref())
            .map(PipeEndpoint::pipe_len)
    }

    fn pipe_set_capacity(&mut self, capacity: usize) -> VfsResult<usize> {
        self.read_end
            .as_ref()
            .or(self.write_end.as_ref())
            .ok_or(VfsError::BadFd)?
            .set_pipe_capacity(capacity)
            .map_err(map_pipe_err)
    }
}

impl VfsIoHandle for PipeReadHandle {
    fn open_accmode(&self) -> u32 {
        0
    }

    fn prepare_read(&mut self, max_len: usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        Ok(Box::new(PipePreparedRead {
            endpoint: self.endpoint.clone(),
            max_len,
        }))
    }

// 本方法代码由AI完成
    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        self.endpoint.read(buf).map_err(map_pipe_err)
    }

// 本方法代码由AI完成
    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
        self.endpoint.poll_revents(events).map_err(map_pipe_err)
    }

// 本方法代码由AI完成
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

// 本方法代码由AI完成
    fn close(&mut self) -> VfsResult<()> {
        self.endpoint.close();
        Ok(())
    }

// 本方法代码由AI完成
    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(special_meta(0o10600, self.inode))
    }

// 本方法代码由AI完成
    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self {
            endpoint: self.endpoint.clone(),
            inode: self.inode,
        }))
    }

// 本方法代码由AI完成
    fn open_status_flags(&self) -> u32 {
        let mut flags = 0;
        if self.endpoint.nonblocking() {
            flags |= O_NONBLOCK;
        }
        if self.endpoint.direct() {
            flags |= O_DIRECT;
        }
        flags
    }

// 本方法代码由AI完成
    fn set_open_status_flags(&mut self, flags: u32) -> VfsResult<()> {
        self.endpoint.set_nonblocking(flags & O_NONBLOCK != 0);
        self.endpoint.set_direct(flags & O_DIRECT != 0);
        Ok(())
    }

// 本方法代码由AI完成
    fn pipe_capacity(&self) -> Option<usize> {
        Some(self.endpoint.pipe_capacity())
    }

// 本方法代码由AI完成
    fn pipe_buffer_len(&self) -> Option<usize> {
        Some(self.endpoint.pipe_len())
    }

// 本方法代码由AI完成
    fn pipe_set_capacity(&mut self, capacity: usize) -> VfsResult<usize> {
        self.endpoint
            .set_pipe_capacity(capacity)
            .map_err(map_pipe_err)
    }
}

/// pipe 写端。
impl VfsIoHandle for PipeWriteHandle {
    fn open_accmode(&self) -> u32 {
        1
    }

// 本方法代码由AI完成
    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        self.endpoint.write(buf).map_err(map_pipe_err)
    }

// 本方法代码由AI完成
    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
        self.endpoint.poll_revents(events).map_err(map_pipe_err)
    }

// 本方法代码由AI完成
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

// 本方法代码由AI完成
    fn close(&mut self) -> VfsResult<()> {
        self.endpoint.close();
        Ok(())
    }

// 本方法代码由AI完成
    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(special_meta(0o10600, self.inode))
    }

// 本方法代码由AI完成
    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self {
            endpoint: self.endpoint.clone(),
            inode: self.inode,
        }))
    }

// 本方法代码由AI完成
    fn open_status_flags(&self) -> u32 {
        let mut flags = 0;
        if self.endpoint.nonblocking() {
            flags |= O_NONBLOCK;
        }
        if self.endpoint.direct() {
            flags |= O_DIRECT;
        }
        flags
    }

// 本方法代码由AI完成
    fn set_open_status_flags(&mut self, flags: u32) -> VfsResult<()> {
        self.endpoint.set_nonblocking(flags & O_NONBLOCK != 0);
        self.endpoint.set_direct(flags & O_DIRECT != 0);
        Ok(())
    }

// 本方法代码由AI完成
    fn pipe_capacity(&self) -> Option<usize> {
        Some(self.endpoint.pipe_capacity())
    }

// 本方法代码由AI完成
    fn pipe_buffer_len(&self) -> Option<usize> {
        Some(self.endpoint.pipe_len())
    }

// 本方法代码由AI完成
    fn pipe_set_capacity(&mut self, capacity: usize) -> VfsResult<usize> {
        self.endpoint
            .set_pipe_capacity(capacity)
            .map_err(map_pipe_err)
    }
}

/// Unix domain stream socket pair 的一端：读/写分别连到交叉 pipe。
// 本结构代码由AI完成
#[derive(Clone)]
pub struct UnixStreamPairEnd {
    read_end: PipeEndpoint,
    write_end: PipeEndpoint,
    inode: u64,
}

// 本方法代码由AI完成
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
    fn prepare_read(&mut self, max_len: usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        Ok(Box::new(PipePreparedRead {
            endpoint: self.read_end.clone(),
            max_len,
        }))
    }

// 本方法代码由AI完成
    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        self.read_end.read(buf).map_err(map_pipe_err)
    }

// 本方法代码由AI完成
    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        self.write_end.write(buf).map_err(map_pipe_err)
    }

// 本方法代码由AI完成
    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
// 本变量代码由AI完成
        const POLLIN: i16 = 0x001;
// 本变量代码由AI完成
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

// 本方法代码由AI完成
    fn poll_wait_for_ticks(
        &mut self,
        events: i16,
        timeout_ticks: u64,
        still_waiting: &mut dyn FnMut() -> bool,
    ) -> VfsResult<()> {
// 本变量代码由AI完成
        const POLLIN: i16 = 0x001;
// 本变量代码由AI完成
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

// 本方法代码由AI完成
    fn close(&mut self) -> VfsResult<()> {
        self.read_end.close();
        self.write_end.close();
        Ok(())
    }

// 本方法代码由AI完成
    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(special_meta(0o140600, self.inode))
    }

// 本方法代码由AI完成
    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self {
            read_end: self.read_end.clone(),
            write_end: self.write_end.clone(),
            inode: self.inode,
        }))
    }

    /// `socketpair` 的两端都可读写。glibc `fdopen(fd, "r+")` 会通过
    /// `fcntl(F_GETFL)` 校验这里返回的访问模式。
    fn open_accmode(&self) -> u32 {
        2
    }

// 本方法代码由AI完成
    fn open_status_flags(&self) -> u32 {
        if self.read_end.nonblocking() {
            O_NONBLOCK
        } else {
            0
        }
    }

// 本方法代码由AI完成
    fn set_open_status_flags(&mut self, flags: u32) -> VfsResult<()> {
        let nonblocking = flags & O_NONBLOCK != 0;
        self.read_end.set_nonblocking(nonblocking);
        self.write_end.set_nonblocking(nonblocking);
        Ok(())
    }
}

struct PipePreparedRead {
    endpoint: PipeEndpoint,
    max_len: usize,
}

impl VfsPreparedRead for PipePreparedRead {
    fn acquire(self: Box<Self>) -> VfsResult<Box<dyn VfsReadLease>> {
        let lease = self
            .endpoint
            .acquire_read_lease(self.max_len)
            .map_err(map_pipe_err)?;
        Ok(Box::new(PipeVfsReadLease { lease }))
    }
}

struct PipeVfsReadLease {
    lease: Box<dyn IpcPipeReadLease>,
}

impl VfsReadLease for PipeVfsReadLease {
    fn bytes(&self) -> &[u8] {
        self.lease.bytes()
    }

    fn finish(self: Box<Self>, progress: VfsCopyProgress) -> VfsResult<VfsReadFinish> {
        let Self { lease } = *self;
        match lease
            .finish(progress.copied, progress.complete)
            .map_err(map_pipe_err)?
        {
            IpcPipeReadFinish::Bytes(copied) => Ok(VfsReadFinish::Bytes(copied)),
            IpcPipeReadFinish::Fault => Ok(VfsReadFinish::Fault),
        }
    }
}

/// bring-up：验证 socketpair 双向读写与 poll。
// 本方法代码由AI完成
pub fn stream_pair_smoke() -> bool {
// 本变量代码由AI完成
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
// 本方法代码由AI完成
pub fn poll_pipe_smoke() -> bool {
// 本变量代码由AI完成
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

// 本方法代码由AI完成
fn map_pipe_err(err: PipeError) -> VfsError {
    match err {
        PipeError::WouldBlock => VfsError::WouldBlock,
        PipeError::Interrupted => VfsError::Interrupted,
        PipeError::BrokenPipe => VfsError::BrokenPipe,
        PipeError::Closed => VfsError::BadFd,
        PipeError::InvalidCapacity => VfsError::Unsupported,
        PipeError::NoMemory => VfsError::NoMemory,
    }
}
