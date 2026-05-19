#![no_std]
//! 管道 v0 实现：固定容量 ring buffer + task waitqueue。
//!
//! 本 crate 保留 `impl-dummy` 包名以维持当前 feature 链，但行为已经是可用的内核内部 pipe。

extern crate alloc;

use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use api_v0::{PipeError, PipeResult, DEFAULT_PIPE_CAPACITY};
use base::sync::UniprocessorSafeCell;
use waitqueue::WaitQueue;

struct PipeState {
    buf : Vec<u8>,
    head : usize,
    len : usize,
    read_open : bool,
    write_open : bool,
}

impl PipeState {
    fn with_capacity(capacity : usize) -> PipeResult<Self> {
        if capacity == 0 {
            return Err(PipeError::InvalidCapacity);
        }
        Ok(Self { buf : vec![0; capacity],
                  head : 0,
                  len : 0,
                  read_open : true,
                  write_open : true })
    }

    #[inline]
    fn capacity(&self) -> usize { self.buf.len() }

    #[inline]
    fn free_len(&self) -> usize { self.capacity().saturating_sub(self.len) }

    #[inline]
    fn is_empty(&self) -> bool { self.len == 0 }

    #[inline]
    fn is_full(&self) -> bool { self.len == self.capacity() }

    fn read_into(&mut self, out : &mut [u8]) -> usize {
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

    fn write_from(&mut self, input : &[u8]) -> usize {
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
    state : UniprocessorSafeCell<PipeState>,
    read_wait : WaitQueue,
    write_wait : WaitQueue,
}

impl Pipe {
    /// 创建默认容量的 pipe。
    #[inline]
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_PIPE_CAPACITY)
            .expect("default pipe capacity must be valid")
    }

    /// 创建指定容量的 pipe。
    pub fn with_capacity(capacity : usize) -> PipeResult<Self> {
        Ok(Self { state : unsafe { UniprocessorSafeCell::new(PipeState::with_capacity(capacity)?) },
                  read_wait : WaitQueue::new(),
                  write_wait : WaitQueue::new() })
    }

    /// 返回缓冲区容量。
    #[inline]
    pub fn capacity(&self) -> usize {
        self.state
            .exclusive_access()
            .capacity()
    }

    /// 返回当前已缓冲字节数。
    #[inline]
    pub fn len(&self) -> usize {
        self.state
            .exclusive_access()
            .len
    }

    /// 非阻塞读取；空且写端未关闭时返回 [`PipeError::WouldBlock`]。
    pub fn try_read(&self, out : &mut [u8]) -> PipeResult<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let mut state = self
            .state
            .exclusive_access();
        if state.is_empty() {
            return if state.write_open {
                Err(PipeError::WouldBlock)
            } else {
                Ok(0)
            };
        }
        let read = state.read_into(out);
        drop(state);
        self.write_wait
            .wake_one();
        Ok(read)
    }

    /// 阻塞读取；空且写端关闭时返回 EOF（`Ok(0)`）。
    pub fn read(&self, out : &mut [u8]) -> PipeResult<usize> {
        loop {
            match self.try_read(out) {
                Err(PipeError::WouldBlock) => {
                    self.read_wait
                        .wait_current_while(|| {
                            let state = self
                                .state
                                .exclusive_access();
                            state.is_empty() && state.write_open
                        });
                }
                other => return other,
            }
        }
    }

    /// 非阻塞写入；满且读端仍打开时返回 [`PipeError::WouldBlock`]。
    pub fn try_write(&self, input : &[u8]) -> PipeResult<usize> {
        if input.is_empty() {
            return Ok(0);
        }
        let mut state = self
            .state
            .exclusive_access();
        if !state.read_open {
            return Err(PipeError::BrokenPipe);
        }
        if state.is_full() {
            return Err(PipeError::WouldBlock);
        }
        let written = state.write_from(input);
        drop(state);
        self.read_wait
            .wake_one();
        Ok(written)
    }

    /// 阻塞写入，尽量写完整个输入缓冲。
    pub fn write(&self, input : &[u8]) -> PipeResult<usize> {
        let mut written = 0usize;
        while written < input.len() {
            match self.try_write(&input[written..]) {
                Ok(0) => break,
                Ok(n) => written = written.saturating_add(n),
                Err(PipeError::WouldBlock) => {
                    self.write_wait
                        .wait_current_while(|| {
                            let state = self
                                .state
                                .exclusive_access();
                            state.is_full() && state.read_open
                        });
                }
                Err(PipeError::BrokenPipe) if written > 0 => return Ok(written),
                Err(err) => return Err(err),
            }
        }
        Ok(written)
    }

    /// 关闭读端并唤醒可能阻塞的写者。
    pub fn close_read(&self) {
        self.state
            .exclusive_access()
            .read_open = false;
        self.write_wait
            .wake_all();
    }

    /// 关闭写端并唤醒可能阻塞的读者。
    pub fn close_write(&self) {
        self.state
            .exclusive_access()
            .write_open = false;
        self.read_wait
            .wake_all();
    }
}

impl Default for Pipe {
    #[inline]
    fn default() -> Self { Self::new() }
}

/// pipe 端点方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeEndpointKind {
    /// 只读端点。
    Read,
    /// 只写端点。
    Write,
}

/// 可放入 fd table 的 pipe 端点。
#[derive(Clone)]
pub struct PipeEndpoint {
    pipe : Arc<Pipe>,
    kind : PipeEndpointKind,
    nonblocking : bool,
}

impl PipeEndpoint {
    /// 创建一对读/写端点。
    pub fn pair(nonblocking : bool) -> (Self, Self) {
        let pipe = Arc::new(Pipe::new());
        (Self { pipe : pipe.clone(),
                kind : PipeEndpointKind::Read,
                nonblocking },
         Self { pipe,
                kind : PipeEndpointKind::Write,
                nonblocking })
    }

    /// 端点方向。
    #[inline]
    pub const fn kind(&self) -> PipeEndpointKind { self.kind }

    /// 是否按非阻塞语义执行。
    #[inline]
    pub const fn nonblocking(&self) -> bool { self.nonblocking }

    /// 从读端读取。
    pub fn read(&self, out : &mut [u8]) -> PipeResult<usize> {
        if self.kind != PipeEndpointKind::Read {
            return Err(PipeError::BrokenPipe);
        }
        if self.nonblocking {
            self.pipe.try_read(out)
        } else {
            self.pipe.read(out)
        }
    }

    /// 从写端写入。
    pub fn write(&self, input : &[u8]) -> PipeResult<usize> {
        if self.kind != PipeEndpointKind::Write {
            return Err(PipeError::BrokenPipe);
        }
        if self.nonblocking {
            self.pipe.try_write(input)
        } else {
            self.pipe.write(input)
        }
    }

    /// 显式关闭该端点。
    pub fn close(&self) {
        match self.kind {
            PipeEndpointKind::Read => self.pipe.close_read(),
            PipeEndpointKind::Write => self.pipe.close_write(),
        }
    }
}
