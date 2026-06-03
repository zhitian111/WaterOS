//! 固定容量 ring buffer pipe 实现。

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use api_v0::{KernelPipe, PipeError, PipeResult};
use base::sync::UniprocessorSafeCell;
use waitqueue::{TaskWaitResult, WaitQueue};

const POLLIN: i16 = 0x001;
const POLLOUT: i16 = 0x004;
const POLLHUP: i16 = 0x010;
const POLLERR: i16 = 0x008;

struct PipeState {
    buf: Vec<u8>,
    head: usize,
    len: usize,
    read_open: bool,
    write_open: bool,
    /// 读端 fd 引用计数；归零时 `read_open` 置 false。
    read_refs: usize,
    /// 写端 fd 引用计数；归零时 `write_open` 置 false。
    write_refs: usize,
}

impl PipeState {
    fn with_capacity(capacity: usize) -> PipeResult<Self> {
        if capacity == 0 {
            return Err(PipeError::InvalidCapacity);
        }
        Ok(Self {
            buf: vec![0; capacity],
            head: 0,
            len: 0,
            read_open: true,
            write_open: true,
            read_refs: 0,
            write_refs: 0,
        })
    }

    #[inline]
    fn capacity(&self) -> usize {
        self.buf.len()
    }

    #[inline]
    fn free_len(&self) -> usize {
        self.capacity().saturating_sub(self.len)
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    fn is_full(&self) -> bool {
        self.len == self.capacity()
    }

    fn read_into(&mut self, out: &mut [u8]) -> usize {
        let count = out.len().min(self.len);
        let capacity = self.capacity();
        for (offset, slot) in out.iter_mut().take(count).enumerate() {
            let idx = (self.head + offset) % capacity;
            *slot = self.buf[idx];
        }
        self.head = (self.head + count) % capacity;
        self.len -= count;
        count
    }

    fn write_from(&mut self, input: &[u8]) -> usize {
        let count = input.len().min(self.free_len());
        let capacity = self.capacity();
        let tail = (self.head + self.len) % capacity;
        for (offset, byte) in input.iter().take(count).enumerate() {
            let idx = (tail + offset) % capacity;
            self.buf[idx] = *byte;
        }
        self.len += count;
        count
    }
}

/// 内核内部 pipe 对象；读写端关闭状态与缓冲区受单核 cell 保护。
pub struct Pipe {
    state: UniprocessorSafeCell<PipeState>,
    read_wait: WaitQueue,
    write_wait: WaitQueue,
}

impl Pipe {
    /// 创建默认容量的 pipe。
    #[inline]
    pub fn new() -> Self {
        <Self as KernelPipe>::new()
    }

    /// 创建指定容量的 pipe。
    pub fn with_capacity(capacity: usize) -> PipeResult<Self> {
        KernelPipe::with_capacity(capacity)
    }

    /// 返回缓冲区容量。
    #[inline]
    pub fn capacity(&self) -> usize {
        KernelPipe::capacity(self)
    }

    /// 返回当前已缓冲字节数。
    #[inline]
    pub fn len(&self) -> usize {
        KernelPipe::len(self)
    }

    /// 非阻塞读取；空且写端未关闭时返回 [`PipeError::WouldBlock`]。
    pub fn try_read(&self, out: &mut [u8]) -> PipeResult<usize> {
        KernelPipe::try_read(self, out)
    }

    /// 阻塞读取；空且写端关闭时返回 EOF（`Ok(0)`）。
    pub fn read(&self, out: &mut [u8]) -> PipeResult<usize> {
        KernelPipe::read(self, out)
    }

    /// 非阻塞写入；满且读端仍打开时返回 [`PipeError::WouldBlock`]。
    pub fn try_write(&self, input: &[u8]) -> PipeResult<usize> {
        KernelPipe::try_write(self, input)
    }

    /// 阻塞写入，尽量写完整个输入缓冲。
    pub fn write(&self, input: &[u8]) -> PipeResult<usize> {
        KernelPipe::write(self, input)
    }

    /// 关闭读端并唤醒可能阻塞的写者。
    pub fn close_read(&self) {
        KernelPipe::close_read(self);
    }

    /// 关闭写端并唤醒可能阻塞的读者。
    pub fn close_write(&self) {
        KernelPipe::close_write(self);
    }

    /// 读端 fd 引用 +1（`dup` / `fork` 继承 / `Clone`）。
    pub fn acquire_read(&self) {
        self.state.exclusive_access().read_refs += 1;
    }

    /// 写端 fd 引用 +1（`dup` / `fork` 继承 / `Clone`）。
    pub fn acquire_write(&self) {
        self.state.exclusive_access().write_refs += 1;
    }

    /// 读端 fd 引用 -1；归零时关闭读端。
    pub fn release_read(&self) {
        let mut state = self.state.exclusive_access();
        if state.read_refs > 0 {
            state.read_refs -= 1;
        }
        if state.read_refs == 0 {
            state.read_open = false;
            drop(state);
            self.write_wait.wake_all();
        }
    }

    /// 写端 fd 引用 -1；归零时关闭写端。
    pub fn release_write(&self) {
        let mut state = self.state.exclusive_access();
        if state.write_refs > 0 {
            state.write_refs -= 1;
        }
        if state.write_refs == 0 {
            state.write_open = false;
            drop(state);
            self.read_wait.wake_all();
        }
    }

    /// 读端在 `poll(2)` 中的就绪位。
    pub fn poll_revents_read(&self) -> i16 {
        let state = self.state.exclusive_access();
        if !state.read_open {
            return POLLHUP;
        }
        let mut revents = 0i16;
        if !state.is_empty() || !state.write_open {
            revents |= POLLIN;
        }
        if !state.write_open {
            revents |= POLLHUP;
        }
        revents
    }

    /// 写端在 `poll(2)` 中的就绪位。
    pub fn poll_revents_write(&self) -> i16 {
        let state = self.state.exclusive_access();
        if !state.write_open {
            return POLLHUP | POLLERR;
        }
        let mut revents = 0i16;
        if state.read_open && !state.is_full() {
            revents |= POLLOUT;
        }
        if !state.read_open {
            revents |= POLLHUP | POLLERR;
        }
        revents
    }

    /// 阻塞直到读端可读或 `still_waiting` 为假（用于多 fd `poll` 重扫）。
    pub fn poll_wait_read_for_ticks(
        &self,
        timeout_ticks: u64,
        still_waiting: &mut dyn FnMut() -> bool,
    ) -> TaskWaitResult {
        self.read_wait.wait_current_while_for_ticks(timeout_ticks, || {
            still_waiting() && self.read_poll_blocked()
        })
    }

    /// 阻塞直到写端可写或 `still_waiting` 为假。
    pub fn poll_wait_write_for_ticks(
        &self,
        timeout_ticks: u64,
        still_waiting: &mut dyn FnMut() -> bool,
    ) -> TaskWaitResult {
        self.write_wait.wait_current_while_for_ticks(timeout_ticks, || {
            still_waiting() && self.write_poll_blocked()
        })
    }

    fn read_poll_blocked(&self) -> bool {
        let state = self.state.exclusive_access();
        state.is_empty() && state.write_open
    }

    fn write_poll_blocked(&self) -> bool {
        let state = self.state.exclusive_access();
        state.is_full() && state.read_open
    }
}

impl Default for Pipe {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl KernelPipe for Pipe {
    fn with_capacity(capacity: usize) -> PipeResult<Self> {
        Ok(Self {
            state: unsafe { UniprocessorSafeCell::new(PipeState::with_capacity(capacity)?) },
            read_wait: WaitQueue::new(),
            write_wait: WaitQueue::new(),
        })
    }

    fn capacity(&self) -> usize {
        self.state.exclusive_access().capacity()
    }

    fn len(&self) -> usize {
        self.state.exclusive_access().len
    }

    fn try_read(&self, out: &mut [u8]) -> PipeResult<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let mut state = self.state.exclusive_access();
        if state.is_empty() {
            return if state.write_open {
                Err(PipeError::WouldBlock)
            } else {
                Ok(0)
            };
        }
        let read = state.read_into(out);
        drop(state);
        self.write_wait.wake_one();
        Ok(read)
    }

    fn read(&self, out: &mut [u8]) -> PipeResult<usize> {
        loop {
            match self.try_read(out) {
                Err(PipeError::WouldBlock) => {
                    self.read_wait.wait_current_while(|| {
                        let state = self.state.exclusive_access();
                        state.is_empty() && state.write_open
                    });
                }
                other => return other,
            }
        }
    }

    fn try_write(&self, input: &[u8]) -> PipeResult<usize> {
        if input.is_empty() {
            return Ok(0);
        }
        let mut state = self.state.exclusive_access();
        if !state.read_open {
            return Err(PipeError::BrokenPipe);
        }
        if state.is_full() {
            return Err(PipeError::WouldBlock);
        }
        let written = state.write_from(input);
        drop(state);
        self.read_wait.wake_one();
        Ok(written)
    }

    fn write(&self, input: &[u8]) -> PipeResult<usize> {
        let mut written = 0usize;
        while written < input.len() {
            match self.try_write(&input[written..]) {
                Ok(0) => break,
                Ok(n) => written = written.saturating_add(n),
                Err(PipeError::WouldBlock) => {
                    self.write_wait.wait_current_while(|| {
                        let state = self.state.exclusive_access();
                        state.is_full() && state.read_open
                    });
                }
                Err(PipeError::BrokenPipe) if written > 0 => return Ok(written),
                Err(err) => return Err(err),
            }
        }
        Ok(written)
    }

    fn close_read(&self) {
        self.state.exclusive_access().read_open = false;
        self.write_wait.wake_all();
    }

    fn close_write(&self) {
        self.state.exclusive_access().write_open = false;
        self.read_wait.wake_all();
    }

    fn poll_revents_read(&self) -> i16 {
        Pipe::poll_revents_read(self)
    }

    fn poll_revents_write(&self) -> i16 {
        Pipe::poll_revents_write(self)
    }
}
