//! 固定容量 ring buffer pipe 实现。
//!
//! `LOCK:` `state` 保护缓冲区和端点引用；每次状态改变后先释放它，再调用 waitqueue。
//! `SMP:` waitqueue 的 wake 路径交给 task scheduler，它决定 ready CPU 与定向 IPI。

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use api_v0::{
    KernelPipe, PipeError, PipeReadFinish, PipeReadLease, PipeResult,
};
use spin::Mutex;
use waitqueue::{TaskWaitResult, WaitQueue};

const POLLIN: i16 = 0x001;
const POLLOUT: i16 = 0x004;
const POLLHUP: i16 = 0x010;
const POLLERR: i16 = 0x008;
const PIPE_BUF: usize = 4096;

#[derive(Clone, Copy)]
struct PipeSegment {
    len: usize,
    packet: bool,
}

#[derive(Clone, Copy)]
struct ReadReservation {
    id: u64,
    exposed: usize,
    consume_on_success: usize,
    packet: bool,
}

/// `DATA:` 同一 pipe 的全部可变状态，必须只在 `Pipe::state` 锁内访问。
struct PipeState {
    buf: Vec<u8>,
    head: usize,
    len: usize,
    segments: VecDeque<PipeSegment>,
    read_reservation: Option<ReadReservation>,
    next_reservation_id: u64,
    /// 最后一个读端引用尚未关闭。
    read_open: bool,
    /// 最后一个写端引用尚未关闭。
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
            segments: VecDeque::new(),
            read_reservation: None,
            next_reservation_id: 1,
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

    fn read_plan(&self, max_len: usize) -> (usize, usize, bool) {
        let Some(segment) = self.segments.front().copied() else {
            return (0, 0, false);
        };
        let exposed = segment.len.min(max_len);
        let consumed = if segment.packet { segment.len } else { exposed };
        (exposed, consumed, segment.packet)
    }

    fn copy_head(&self, out: &mut [u8]) {
        let count = out.len().min(self.len);
        let capacity = self.capacity();
        let first_run = (capacity - self.head).min(count);
        out[..first_run].copy_from_slice(&self.buf[self.head..self.head + first_run]);
        let remaining = count - first_run;
        if remaining > 0 {
            out[first_run..count].copy_from_slice(&self.buf[..remaining]);
        }
    }

    fn consume(&mut self, mut count: usize) {
        count = count.min(self.len);
        if count == 0 {
            return;
        }
        let consumed = count;
        while count > 0 {
            let Some(front) = self.segments.front_mut() else {
                break;
            };
            let take = count.min(front.len);
            front.len -= take;
            count -= take;
            if front.len == 0 {
                self.segments.pop_front();
            }
        }
        let capacity = self.capacity();
        self.head = (self.head + consumed) % capacity;
        self.len -= consumed;
    }

    /// 从 ring buffer 的 head 开始读；packet 的未返回尾部一并丢弃。
    fn read_into(&mut self, out: &mut [u8]) -> usize {
        let (exposed, consumed, _) = self.read_plan(out.len());
        if exposed == 0 {
            return 0;
        }
        self.copy_head(&mut out[..exposed]);
        self.consume(consumed);
        exposed
    }

    fn record_segment(&mut self, len: usize, packet: bool) {
        if len == 0 {
            return;
        }
        if !packet {
            if let Some(back) = self.segments.back_mut() {
                if !back.packet {
                    back.len += len;
                    return;
                }
            }
        }
        self.segments.push_back(PipeSegment { len, packet });
    }

    /// 从 `(head + len) % capacity` 写入；调用方已确认存在空位。
    fn write_from(&mut self, input: &[u8], packet: bool) -> usize {
        let count = input.len().min(self.free_len());
        if count == 0 {
            return 0;
        }
        let capacity = self.capacity();
        let tail = (self.head + self.len) % capacity;
        let first_run = (capacity - tail).min(count);
        self.buf[tail..tail + first_run].copy_from_slice(&input[..first_run]);
        let remaining = count - first_run;
        if remaining > 0 {
            self.buf[..remaining].copy_from_slice(&input[first_run..count]);
        }
        self.len += count;
        self.record_segment(count, packet);
        count
    }

    fn begin_read(&mut self, staging: &mut Vec<u8>) -> PipeResult<ReadReservation> {
        if self.read_reservation.is_some() {
            return Err(PipeError::WouldBlock);
        }
        let (exposed, consume_on_success, packet) = self.read_plan(staging.capacity());
        staging.resize(exposed, 0);
        self.copy_head(staging.as_mut_slice());
        let reservation = ReadReservation {
            id: self.next_reservation_id,
            exposed,
            consume_on_success,
            packet,
        };
        self.next_reservation_id = self.next_reservation_id.wrapping_add(1).max(1);
        self.read_reservation = Some(reservation);
        Ok(reservation)
    }

    fn finish_read(
        &mut self,
        id: u64,
        copied: usize,
        complete: bool,
    ) -> PipeResult<(PipeReadFinish, bool)> {
        let reservation = self.read_reservation.ok_or(PipeError::Closed)?;
        if reservation.id != id || copied > reservation.exposed {
            return Err(PipeError::Closed);
        }
        let (finish, consumed) = if reservation.packet {
            if complete {
                (PipeReadFinish::Bytes(copied), reservation.consume_on_success)
            } else if copied == 0 {
                (PipeReadFinish::Fault, 0)
            } else {
                (PipeReadFinish::Fault, reservation.consume_on_success)
            }
        } else if copied == 0 && !complete {
            (PipeReadFinish::Fault, 0)
        } else {
            (PipeReadFinish::Bytes(copied), copied)
        };
        self.read_reservation = None;
        self.consume(consumed);
        Ok((finish, consumed != 0))
    }

    fn cancel_read(&mut self, id: u64) {
        if self.read_reservation.map(|entry| entry.id) == Some(id) {
            self.read_reservation = None;
        }
    }
}

/// `DATA:` 内核内部 pipe 对象；缓冲和端点开放状态由 `state` 保护。
///
/// 两个等待队列按等待条件分离：读者等待“非空或写端关闭”，写者等待“未满或读端关闭”。
pub(crate) struct Pipe {
    state: Mutex<PipeState>,
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

    /// 调整缓冲区容量；仅当管道为空时可缩小/扩大底层缓冲。
    pub fn set_capacity(&self, capacity: usize) -> PipeResult<usize> {
        KernelPipe::set_capacity(self, capacity)
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

    pub fn acquire_read_lease(
        self: &Arc<Self>,
        max_len: usize,
        nonblocking: bool,
    ) -> PipeResult<Box<dyn PipeReadLease>> {
        let allocation = max_len.min(self.capacity());
        let mut staging = Vec::new();
        staging
            .try_reserve_exact(allocation)
            .map_err(|_| PipeError::NoMemory)?;
        loop {
            let mut state = self.state.lock();
            if state.read_reservation.is_none() && !state.is_empty() {
                let reservation = state.begin_read(&mut staging)?;
                drop(state);
                return Ok(Box::new(PipeReadLeaseImpl {
                    pipe: self.clone(),
                    reservation_id: Some(reservation.id),
                    data: staging,
                }));
            }
            if state.read_reservation.is_none() && !state.write_open {
                drop(state);
                return Ok(Box::new(PipeReadLeaseImpl {
                    pipe: self.clone(),
                    reservation_id: None,
                    data: staging,
                }));
            }
            if nonblocking {
                return Err(PipeError::WouldBlock);
            }
            drop(state);
            let result = self.read_wait.wait_current_while(|| {
                let state = self.state.lock();
                state.read_reservation.is_some() || (state.is_empty() && state.write_open)
            });
            if result == TaskWaitResult::Interrupted {
                return Err(PipeError::Interrupted);
            }
        }
    }

    /// 非阻塞写入；满且读端仍打开时返回 [`PipeError::WouldBlock`]。
    pub fn try_write(&self, input: &[u8]) -> PipeResult<usize> {
        self.try_write_mode(input, false)
    }

    pub fn try_write_mode(&self, input: &[u8], packet: bool) -> PipeResult<usize> {
        if input.is_empty() {
            return Ok(0);
        }
        let mut state = self.state.lock();
        if !state.read_open {
            return Err(PipeError::BrokenPipe);
        }
        let packet_len = input.len().min(PIPE_BUF).min(state.capacity());
        if state.is_full() || (packet && state.free_len() < packet_len) {
            return Err(PipeError::WouldBlock);
        }
        let written = state.write_from(
            if packet { &input[..packet_len] } else { input },
            packet,
        );
        drop(state);
        self.read_wait.wake_all();
        Ok(written)
    }

    pub fn write_mode(&self, input: &[u8], packet: bool) -> PipeResult<usize> {
        let mut written = 0usize;
        while written < input.len() {
            match self.try_write_mode(&input[written..], packet) {
                Ok(0) => break,
                Ok(n) => written = written.saturating_add(n),
                Err(PipeError::WouldBlock) => {
                    let result = self.write_wait.wait_current_while(|| {
                        let state = self.state.lock();
                        let needed = if packet {
                            (input.len() - written).min(PIPE_BUF).min(state.capacity())
                        } else {
                            1
                        };
                        state.free_len() < needed && state.read_open
                    });
                    if result == TaskWaitResult::Interrupted {
                        return if written == 0 {
                            Err(PipeError::Interrupted)
                        } else {
                            Ok(written)
                        };
                    }
                }
                Err(PipeError::BrokenPipe) if written > 0 => return Ok(written),
                Err(err) => return Err(err),
            }
        }
        Ok(written)
    }

    /// 读端 fd 引用 +1（`dup` / `fork` 继承 / `Clone`）。
    pub fn acquire_read(&self) {
        self.state.lock().read_refs += 1;
    }

    /// 写端 fd 引用 +1（`dup` / `fork` 继承 / `Clone`）。
    pub fn acquire_write(&self) {
        self.state.lock().write_refs += 1;
    }

    /// `FLOW:` 读端 fd 引用 -1；归零时关闭读端并在锁外唤醒所有写者。
    pub fn release_read(&self) {
        let mut state = self.state.lock();
        if state.read_refs > 0 {
            state.read_refs -= 1;
        }
        if state.read_refs == 0 {
            state.read_open = false;
            state.read_reservation = None;
            drop(state);
            self.read_wait.wake_all();
            self.write_wait.wake_all();
        }
    }

    /// `FLOW:` 写端 fd 引用 -1；归零时关闭写端并在锁外唤醒所有读者。
    pub fn release_write(&self) {
        let mut state = self.state.lock();
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
        let state = self.state.lock();
        if !state.read_open {
            return POLLHUP;
        }
        let mut revents = 0i16;
        if state.read_reservation.is_none() && (!state.is_empty() || !state.write_open) {
            revents |= POLLIN;
        }
        if !state.write_open {
            revents |= POLLHUP;
        }
        revents
    }

    /// 写端在 `poll(2)` 中的就绪位。
    pub fn poll_revents_write(&self) -> i16 {
        let state = self.state.lock();
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

    /// `FLOW:` 阻塞直到读端可读、最后一个写端关闭，或 `still_waiting` 为假。
    /// 条件由 scheduler 原子复查，避免 poll 注册与写入之间丢失唤醒。
    pub fn poll_wait_read_for_ticks(
        &self,
        timeout_ticks: u64,
        still_waiting: &mut dyn FnMut() -> bool,
    ) -> TaskWaitResult {
        self.read_wait.wait_current_while_for_ticks(timeout_ticks, || {
            still_waiting() && self.read_poll_blocked()
        })
    }

    /// `FLOW:` 阻塞直到写端可写、读端关闭，或 `still_waiting` 为假。
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
        let state = self.state.lock();
        state.read_reservation.is_some() || (state.is_empty() && state.write_open)
    }

    fn write_poll_blocked(&self) -> bool {
        let state = self.state.lock();
        state.is_full() && state.read_open
    }
}

struct PipeReadLeaseImpl {
    pipe: Arc<Pipe>,
    reservation_id: Option<u64>,
    data: Vec<u8>,
}

impl PipeReadLease for PipeReadLeaseImpl {
    fn bytes(&self) -> &[u8] {
        self.data.as_slice()
    }

    fn finish(
        mut self: Box<Self>,
        copied: usize,
        complete: bool,
    ) -> PipeResult<PipeReadFinish> {
        let Some(id) = self.reservation_id.take() else {
            return Ok(PipeReadFinish::Bytes(0));
        };
        let mut state = self.pipe.state.lock();
        let (finish, freed) = state.finish_read(id, copied, complete)?;
        drop(state);
        self.pipe.read_wait.wake_all();
        if freed {
            self.pipe.write_wait.wake_all();
        }
        Ok(finish)
    }
}

impl Drop for PipeReadLeaseImpl {
    fn drop(&mut self) {
        let Some(id) = self.reservation_id.take() else {
            return;
        };
        self.pipe.state.lock().cancel_read(id);
        self.pipe.read_wait.wake_all();
        self.pipe.write_wait.wake_all();
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
            state: Mutex::new(PipeState::with_capacity(capacity)?),
            read_wait: WaitQueue::new_named("pipe-read"),
            write_wait: WaitQueue::new_named("pipe-write"),
        })
    }

    fn capacity(&self) -> usize {
        self.state.lock().capacity()
    }

    fn set_capacity(&self, capacity: usize) -> PipeResult<usize> {
        if capacity == 0 {
            return Err(PipeError::InvalidCapacity);
        }
        let mut state = self.state.lock();
        if state.len != 0 {
            return Err(PipeError::InvalidCapacity);
        }
        state.buf = vec![0; capacity];
        state.head = 0;
        state.segments.clear();
        Ok(capacity)
    }

    fn len(&self) -> usize {
        self.state.lock().len
    }

    /// `FLOW:` 在锁内取出字节，解锁后通知写者；空且写端关闭返回 EOF。
    fn try_read(&self, out: &mut [u8]) -> PipeResult<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let mut state = self.state.lock();
        if state.read_reservation.is_some() {
            return Err(PipeError::WouldBlock);
        }
        if state.is_empty() {
            return if state.write_open {
                Err(PipeError::WouldBlock)
            } else {
                Ok(0)
            };
        }
        let read = state.read_into(out);
        drop(state);
        self.write_wait.wake_all();
        Ok(read)
    }

    /// `FLOW:` 条件等待与 `try_read` 重试；不持有 state 锁进入 scheduler。
    fn read(&self, out: &mut [u8]) -> PipeResult<usize> {
        loop {
            match self.try_read(out) {
                Err(PipeError::WouldBlock) => {
                    let result = self.read_wait.wait_current_while(|| {
                        let state = self.state.lock();
                        state.read_reservation.is_some() ||
                        (state.is_empty() && state.write_open)
                    });
                    if result == TaskWaitResult::Interrupted {
                        return Err(PipeError::Interrupted);
                    }
                }
                other => return other,
            }
        }
    }

    /// `FLOW:` 在锁内写入字节，解锁后通知读者；读端关闭优先返回 `BrokenPipe`。
    fn try_write(&self, input: &[u8]) -> PipeResult<usize> {
        if input.is_empty() {
            return Ok(0);
        }
        let mut state = self.state.lock();
        if !state.read_open {
            return Err(PipeError::BrokenPipe);
        }
        if state.is_full() {
            return Err(PipeError::WouldBlock);
        }
        let written = state.write_from(input, false);
        drop(state);
        self.read_wait.wake_all();
        Ok(written)
    }

    /// `FLOW:` 尽量完成整次写入；部分成功后被中断或遇到断管返回已写部分。
    fn write(&self, input: &[u8]) -> PipeResult<usize> {
        let mut written = 0usize;
        while written < input.len() {
            match self.try_write(&input[written..]) {
                Ok(0) => break,
                Ok(n) => written = written.saturating_add(n),
                Err(PipeError::WouldBlock) => {
                    let result = self.write_wait.wait_current_while(|| {
                        let state = self.state.lock();
                        state.is_full() && state.read_open
                    });
                    if result == TaskWaitResult::Interrupted {
                        return if written == 0 {
                            Err(PipeError::Interrupted)
                        } else {
                            Ok(written)
                        };
                    }
                }
                Err(PipeError::BrokenPipe) if written > 0 => return Ok(written),
                Err(err) => return Err(err),
            }
        }
        Ok(written)
    }

    fn poll_revents_read(&self) -> i16 {
        Pipe::poll_revents_read(self)
    }

    fn poll_revents_write(&self) -> i16 {
        Pipe::poll_revents_write(self)
    }
}
