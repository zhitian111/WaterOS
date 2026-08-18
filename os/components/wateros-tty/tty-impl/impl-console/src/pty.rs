//! UNIX98 伪终端核心。
//!
//! 本模块只管理字节流、行规程和终端归属，不复制用户指针，也不直接投递信号。
//! 控制字符产生的事件由 syscall 层在释放 PTY 锁后消费。

extern crate alloc;

use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;
use waitqueue::{TaskWaitResult, WaitQueue};

use api_v0::*;

const MAX_PTYS: u32 = 64;
const QUEUE_CAPACITY: usize = 64 * 1024;
static NEXT_TERMINAL_ID: AtomicU64 = AtomicU64::new(2);

struct PtyRegistry {
    pairs: BTreeMap<u32, Weak<SharedTerminal>>,
    sessions: BTreeMap<usize, TerminalId>,
}

impl PtyRegistry {
    const fn new() -> Self {
        Self { pairs: BTreeMap::new(), sessions: BTreeMap::new() }
    }
}

static REGISTRY: Mutex<PtyRegistry> = Mutex::new(PtyRegistry::new());

struct PtyState {
    /// slave 是否仍处于锁定状态。
    locked: bool,
    /// 当前行规程配置。
    termios: TtyTermios,
    /// 窗口尺寸。
    winsize: TtyWinSize,
    /// 前台进程组。
    foreground_pgid: usize,
    /// 控制终端所属会话。
    controlling_sid: usize,
    /// master 写入、经过 slave 行规程后可由 slave 读取的数据。
    slave_readable: VecDeque<u8>,
    slave_editing: Vec<u8>,
    /// slave 端待交付的 EOF。
    slave_eof_pending: bool,
    /// slave 输出和行规程回显，供 master（nxterm）读取。
    master_readable: VecDeque<u8>,
    master_read_id: Option<u64>,
    slave_read_id: Option<u64>,
    /// 下一个读预约序号。
    next_read_id: u64,
    /// master 打开文件描述数量。
    master_open_descriptions: usize,
    /// slave 打开文件描述数量。
    slave_open_descriptions: usize,
    /// slave 已挂断，master 读取应观察到 EOF/HUP。
    master_hung_up: bool,
    /// master 已挂断，slave 读取应观察到 EOF/HUP。
    slave_hung_up: bool,
    /// 行规程产生、待 syscall 层投递的控制信号事件。
    events: VecDeque<TtyControlEvent>,
}

impl PtyState {
    fn new() -> Self {
        Self {
            locked: true,
            termios: TtyTermios::DEFAULT,
            winsize: TtyWinSize::DEFAULT,
            foreground_pgid: 0,
            controlling_sid: 0,
            slave_readable: VecDeque::new(),
            slave_editing: Vec::new(),
            slave_eof_pending: false,
            master_readable: VecDeque::new(),
            master_read_id: None,
            slave_read_id: None,
            next_read_id: 1,
            master_open_descriptions: 1,
            slave_open_descriptions: 0,
            master_hung_up: false,
            slave_hung_up: false,
            events: VecDeque::new(),
        }
    }

    fn canonical(&self) -> bool { self.termios.lflag & ICANON != 0 }
    fn echo(&self) -> bool { self.termios.lflag & ECHO != 0 }
    fn signals(&self) -> bool { self.termios.lflag & ISIG != 0 }

    fn slave_readable_for(&self, max_len: usize) -> bool {
        if self.slave_eof_pending || self.master_hung_up {
            return true;
        }
        if self.canonical() {
            return !self.slave_readable.is_empty();
        }
        let minimum = usize::from(self.termios.cc[VMIN]).min(max_len);
        if minimum == 0 {
            !self.slave_readable.is_empty() || self.termios.cc[VTIME] == 0
        } else {
            self.slave_readable.len() >= minimum
        }
    }

    fn master_readable_now(&self) -> bool {
        !self.master_readable.is_empty() || self.slave_hung_up
    }

    fn push_master_bytes(&mut self, bytes: &[u8]) {
        let available = QUEUE_CAPACITY.saturating_sub(self.master_readable.len());
        self.master_readable.extend(bytes.iter().copied().take(available));
    }

    fn emit_signal(&mut self, signal: usize) {
        if self.foreground_pgid != 0 {
            self.events.push_back(TtyControlEvent {
                process_group: self.foreground_pgid,
                signal,
            });
        }
    }

    /// 把 master 送来的一个按键交给 slave 行规程。
    fn feed_master_byte(&mut self, mut byte: u8) {
        if byte == b'\r' && self.termios.iflag & ICRNL != 0 {
            byte = b'\n';
        }
        if self.signals() {
            let signal = if byte == self.termios.cc[VINTR] {
                Some(SIGINT)
            } else if byte == self.termios.cc[VQUIT] {
                Some(SIGQUIT)
            } else if byte == self.termios.cc[VSUSP] {
                Some(SIGTSTP)
            } else {
                None
            };
            if let Some(signal) = signal {
                self.slave_editing.clear();
                if self.echo() {
                    let marker = match signal { SIGINT => b'C', SIGQUIT => b'\\', _ => b'Z' };
                    self.push_master_bytes(&[b'^', marker, b'\r', b'\n']);
                }
                self.emit_signal(signal);
                return;
            }
        }

        if self.canonical() {
            if byte == self.termios.cc[VERASE] || byte == 8 {
                if self.slave_editing.pop().is_some() && self.echo() {
                    self.push_master_bytes(b"\x08 \x08");
                }
            } else if byte == self.termios.cc[VKILL] {
                self.slave_editing.clear();
                if self.echo() { self.push_master_bytes(b"^U\r\n"); }
            } else if byte == self.termios.cc[VEOF] {
                if self.slave_editing.is_empty() {
                    self.slave_eof_pending = true;
                } else {
                    let editing = core::mem::take(&mut self.slave_editing);
                    self.slave_readable.extend(editing);
                }
            } else if byte == b'\n' {
                let editing = core::mem::take(&mut self.slave_editing);
                self.slave_readable.extend(editing);
                if self.slave_readable.len() < QUEUE_CAPACITY {
                    self.slave_readable.push_back(b'\n');
                }
                if self.echo() { self.push_master_bytes(b"\r\n"); }
            } else if self.slave_editing.len() + self.slave_readable.len() < QUEUE_CAPACITY {
                self.slave_editing.push(byte);
                if self.echo() { self.push_master_bytes(&[byte]); }
            }
        } else if self.slave_readable.len() < QUEUE_CAPACITY {
            self.slave_readable.push_back(byte);
            if self.echo() { self.push_master_bytes(&[byte]); }
        }
    }
}

/// 一个可共享的 PTY 终端实例。
///
/// `Arc<SharedTerminal>` 由 master/slave 打开文件描述共同持有；内部短锁只保护
/// 行规程和有界队列，不允许跨用户复制、等待或信号投递。
pub struct SharedTerminal {
    id: TerminalId,
    number: u32,
    state: Mutex<PtyState>,
    master_wait: WaitQueue,
    slave_wait: WaitQueue,
    master_space_wait: WaitQueue,
    slave_space_wait: WaitQueue,
}

impl SharedTerminal {
    fn new(number: u32) -> Arc<Self> {
        Arc::new(Self {
            id: TerminalId::from_raw(NEXT_TERMINAL_ID.fetch_add(1, Ordering::Relaxed)),
            number,
            state: Mutex::new(PtyState::new()),
            master_wait: WaitQueue::new_named("pty-master-read"),
            slave_wait: WaitQueue::new_named("pty-slave-read"),
            master_space_wait: WaitQueue::new_named("pty-master-space"),
            slave_space_wait: WaitQueue::new_named("pty-slave-space"),
        })
    }
}

impl Drop for SharedTerminal {
    fn drop(&mut self) {
        let mut registry = REGISTRY.lock();
        registry.pairs.remove(&self.number);
        registry.sessions.retain(|_, terminal| *terminal != self.id);
        let _ = self.master_wait.try_release_empty();
        let _ = self.slave_wait.try_release_empty();
        let _ = self.master_space_wait.try_release_empty();
        let _ = self.slave_space_wait.try_release_empty();
    }
}

struct EndpointLease {
    pair: Arc<SharedTerminal>,
    endpoint: TerminalEndpoint,
}

impl Drop for EndpointLease {
    fn drop(&mut self) {
        let (wake_master, wake_slave) = {
            let mut state = self.pair.state.lock();
            match self.endpoint {
                TerminalEndpoint::PtyMaster => {
                    state.master_open_descriptions = state.master_open_descriptions.saturating_sub(1);
                    if state.master_open_descriptions == 0 {
                        state.master_hung_up = true;
                        state.emit_signal(SIGHUP);
                        state.emit_signal(SIGCONT);
                    }
                }
                TerminalEndpoint::PtySlave => {
                    state.slave_open_descriptions = state.slave_open_descriptions.saturating_sub(1);
                    if state.slave_open_descriptions == 0 { state.slave_hung_up = true; }
                }
                TerminalEndpoint::Console => {}
            }
            (state.slave_hung_up, state.master_hung_up)
        };
        if wake_master { self.pair.master_wait.wake_all(); }
        if wake_slave { self.pair.slave_wait.wake_all(); }
        self.pair.master_space_wait.wake_all();
        self.pair.slave_space_wait.wake_all();
    }
}

/// VFS 打开文件描述持有的一个 PTY 端点。
pub struct PtyEndpointHandle {
    pair: Arc<SharedTerminal>,
    endpoint: TerminalEndpoint,
    lease: Arc<EndpointLease>,
    nonblocking: Arc<AtomicBool>,
    accmode: u32,
}

impl Clone for PtyEndpointHandle {
    fn clone(&self) -> Self {
        Self {
            pair: self.pair.clone(),
            endpoint: self.endpoint,
            lease: self.lease.clone(),
            nonblocking: self.nonblocking.clone(),
            accmode: self.accmode,
        }
    }
}

impl PtyEndpointHandle {
    fn new(pair: Arc<SharedTerminal>, endpoint: TerminalEndpoint, accmode: u32, nonblocking: bool) -> Self {
        Self {
            lease: Arc::new(EndpointLease { pair: pair.clone(), endpoint }),
            pair,
            endpoint,
            nonblocking: Arc::new(AtomicBool::new(nonblocking)),
            accmode,
        }
    }

    pub fn id(&self) -> TerminalId { self.pair.id }
    pub fn number(&self) -> u32 { self.pair.number }
    pub fn endpoint(&self) -> TerminalEndpoint { self.endpoint }
    pub fn accmode(&self) -> u32 { self.accmode }
    pub fn nonblocking(&self) -> bool { self.nonblocking.load(Ordering::Acquire) }
    pub fn set_nonblocking(&self, value: bool) { self.nonblocking.store(value, Ordering::Release); }

    pub fn locked(&self) -> bool { self.pair.state.lock().locked }
    pub fn set_locked(&self, locked: bool) { self.pair.state.lock().locked = locked; }

    pub fn termios(&self) -> TtyTermios { self.pair.state.lock().termios }
    pub fn set_termios(&self, termios: TtyTermios, flush_input: bool) {
        let mut state = self.pair.state.lock();
        let was_canonical = state.canonical();
        if flush_input {
            state.slave_readable.clear();
            state.slave_editing.clear();
            state.slave_eof_pending = false;
        } else if was_canonical && termios.lflag & ICANON == 0 {
            let editing = core::mem::take(&mut state.slave_editing);
            state.slave_readable.extend(editing);
        }
        state.termios = termios;
        drop(state);
        self.pair.slave_wait.wake_all();
    }
    pub fn winsize(&self) -> TtyWinSize { self.pair.state.lock().winsize }
    pub fn set_winsize(&self, value: TtyWinSize) {
        self.pair.state.lock().winsize = value;
    }
    pub fn foreground_pgid(&self) -> usize { self.pair.state.lock().foreground_pgid }
    pub fn set_foreground_pgid(&self, pgid: usize) { self.pair.state.lock().foreground_pgid = pgid; }
    pub fn controlling_sid(&self) -> usize { self.pair.state.lock().controlling_sid }
    pub fn output_stops_background(&self) -> bool {
        self.pair.state.lock().termios.lflag & TOSTOP != 0
    }
    pub fn readable_len(&self) -> usize {
        let state = self.pair.state.lock();
        match self.endpoint {
            TerminalEndpoint::PtyMaster => state.master_readable.len(),
            TerminalEndpoint::PtySlave => state.slave_readable.len(),
            TerminalEndpoint::Console => 0,
        }
    }
    pub fn read_settings(&self) -> (bool, usize, u64) {
        let state = self.pair.state.lock();
        (state.canonical(), usize::from(state.termios.cc[VMIN]), u64::from(state.termios.cc[VTIME]))
    }

    pub fn poll_readable(&self) -> bool {
        let state = self.pair.state.lock();
        match self.endpoint {
            TerminalEndpoint::PtyMaster => state.master_readable_now(),
            TerminalEndpoint::PtySlave => state.slave_readable_for(usize::MAX),
            TerminalEndpoint::Console => false,
        }
    }

    pub fn poll_writable(&self) -> bool {
        let state = self.pair.state.lock();
        match self.endpoint {
            // slave 尚未首次 open 时，Linux PTY 允许 master 先写并暂存输入；
            // 只有 slave 曾打开且最后一个描述关闭（slave_hung_up）后才报告挂断。
            TerminalEndpoint::PtyMaster => !state.slave_hung_up &&
                state.slave_readable.len() + state.slave_editing.len() < QUEUE_CAPACITY,
            TerminalEndpoint::PtySlave => !state.master_hung_up &&
                state.master_open_descriptions != 0 &&
                state.master_readable.len() < QUEUE_CAPACITY,
            TerminalEndpoint::Console => false,
        }
    }

    /// 对端最后一个打开文件描述已经关闭。
    pub fn poll_hung_up(&self) -> bool {
        let state = self.pair.state.lock();
        match self.endpoint {
            TerminalEndpoint::PtyMaster => state.slave_hung_up,
            TerminalEndpoint::PtySlave => state.master_hung_up,
            TerminalEndpoint::Console => false,
        }
    }

    pub fn prepare_read(&self, max_len: usize, partial: bool) -> PtyPreparedRead {
        let mut state = self.pair.state.lock();
        let (queue, active, hungup, eof) = match self.endpoint {
            TerminalEndpoint::PtyMaster => (
                &mut state.master_readable as *mut VecDeque<u8>,
                &mut state.master_read_id as *mut Option<u64>,
                state.slave_hung_up,
                false,
            ),
            TerminalEndpoint::PtySlave => (
                &mut state.slave_readable as *mut VecDeque<u8>,
                &mut state.slave_read_id as *mut Option<u64>,
                state.master_hung_up,
                state.slave_eof_pending,
            ),
            TerminalEndpoint::Console => return PtyPreparedRead::Pending,
        };
        // 上面的裸指针只用于同时访问同一锁保护对象的两个不重叠字段。
        let queue = unsafe { &mut *queue };
        let active = unsafe { &mut *active };
        if active.is_some() { return PtyPreparedRead::Pending; }
        let ready = if partial {
            !queue.is_empty()
        } else {
            match self.endpoint {
                TerminalEndpoint::PtyMaster => !queue.is_empty() || hungup,
                TerminalEndpoint::PtySlave => state.slave_readable_for(max_len),
                TerminalEndpoint::Console => false,
            }
        };
        if !ready { return PtyPreparedRead::Pending; }
        if queue.is_empty() && hungup && self.endpoint == TerminalEndpoint::PtyMaster {
            return PtyPreparedRead::HungUp;
        }
        if queue.is_empty() && (hungup || eof) {
            if eof { state.slave_eof_pending = false; }
            return PtyPreparedRead::Eof;
        }
        let len = max_len.min(queue.len());
        let mut bytes = Vec::new();
        if bytes.try_reserve_exact(len).is_err() { return PtyPreparedRead::Pending; }
        for _ in 0..len { bytes.push(queue.pop_front().expect("PTY queue length checked")); }
        let id = state.next_read_id;
        state.next_read_id = state.next_read_id.wrapping_add(1);
        *active = Some(id);
        PtyPreparedRead::Data(PtyReadReservation {
            pair: self.pair.clone(), endpoint: self.endpoint, id, bytes, finished: false,
        })
    }

    pub fn wait_readable(&self, max_len: usize) -> TaskWaitResult {
        match self.endpoint {
            TerminalEndpoint::PtyMaster => self.pair.master_wait.wait_current_while(|| {
                let state = self.pair.state.lock();
                state.master_readable.is_empty() && !state.slave_hung_up
            }),
            TerminalEndpoint::PtySlave => self.pair.slave_wait.wait_current_while(|| {
                !self.pair.state.lock().slave_readable_for(max_len)
            }),
            TerminalEndpoint::Console => TaskWaitResult::Interrupted,
        }
    }

    pub fn wait_readable_for_ticks(&self, max_len: usize, ticks: u64) -> TaskWaitResult {
        match self.endpoint {
            TerminalEndpoint::PtyMaster => self.pair.master_wait.wait_current_while_for_ticks(ticks, || {
                let state = self.pair.state.lock();
                state.master_readable.is_empty() && !state.slave_hung_up
            }),
            TerminalEndpoint::PtySlave => self.pair.slave_wait.wait_current_while_for_ticks(ticks, || {
                !self.pair.state.lock().slave_readable_for(max_len)
            }),
            TerminalEndpoint::Console => TaskWaitResult::Interrupted,
        }
    }

    /// 等待当前端点的目标队列出现空间。
    ///
    /// master 写入的是 slave 输入队列，slave 写入的是 master 输出队列，
    /// 因此两个方向分别使用独立 waitqueue。调用期间不会持有 PTY 状态锁。
    pub fn wait_writable(&self) -> TaskWaitResult {
        match self.endpoint {
            TerminalEndpoint::PtyMaster => self.pair.slave_space_wait.wait_current_while(|| {
                !self.poll_writable()
            }),
            TerminalEndpoint::PtySlave => self.pair.master_space_wait.wait_current_while(|| {
                !self.poll_writable()
            }),
            TerminalEndpoint::Console => TaskWaitResult::Interrupted,
        }
    }

    pub fn write(&self, input: &[u8]) -> Result<usize, PtyError> {
        if input.is_empty() { return Ok(0); }
        let mut state = self.pair.state.lock();
        match self.endpoint {
            TerminalEndpoint::PtyMaster => {
                if state.slave_hung_up {
                    return Err(PtyError::HungUp);
                }
                let before = state.slave_readable.len() + state.slave_editing.len();
                let capacity = QUEUE_CAPACITY.saturating_sub(before);
                if capacity == 0 { return Err(PtyError::WouldBlock); }
                let count = input.len().min(capacity);
                for byte in &input[..count] { state.feed_master_byte(*byte); }
                drop(state);
                self.pair.slave_wait.wake_all();
                self.pair.master_wait.wake_all();
                Ok(count)
            }
            TerminalEndpoint::PtySlave => {
                if state.slave_hung_up || state.master_open_descriptions == 0 {
                    return Err(PtyError::HungUp);
                }
                let available = QUEUE_CAPACITY.saturating_sub(state.master_readable.len());
                if available == 0 { return Err(PtyError::WouldBlock); }
                let mut consumed = 0;
                for byte in input {
                    let expands = *byte == b'\n' && state.termios.oflag & OPOST != 0 &&
                        state.termios.oflag & ONLCR != 0;
                    let need = if expands { 2 } else { 1 };
                    if state.master_readable.len().saturating_add(need) > QUEUE_CAPACITY { break; }
                    if expands { state.master_readable.push_back(b'\r'); }
                    state.master_readable.push_back(*byte);
                    consumed += 1;
                }
                if consumed == 0 { return Err(PtyError::WouldBlock); }
                drop(state);
                self.pair.master_wait.wake_all();
                Ok(consumed)
            }
            TerminalEndpoint::Console => Err(PtyError::Invalid),
        }
    }
}

/// 已从 PTY 队列临时移出的字节；用户复制结束后必须提交或回滚。
pub struct PtyReadReservation {
    pair: Arc<SharedTerminal>,
    endpoint: TerminalEndpoint,
    id: u64,
    bytes: Vec<u8>,
    finished: bool,
}

impl PtyReadReservation {
    pub fn bytes(&self) -> &[u8] { &self.bytes }

    pub fn finish(mut self, copied: usize, complete: bool) -> Result<usize, PtyError> {
        if copied > self.bytes.len() { return Err(PtyError::Invalid); }
        let mut state = self.pair.state.lock();
        match self.endpoint {
            TerminalEndpoint::PtyMaster => {
                if state.master_read_id != Some(self.id) { return Err(PtyError::Invalid); }
                for byte in self.bytes[copied..].iter().rev() {
                    state.master_readable.push_front(*byte);
                }
                state.master_read_id = None;
            }
            TerminalEndpoint::PtySlave => {
                if state.slave_read_id != Some(self.id) { return Err(PtyError::Invalid); }
                for byte in self.bytes[copied..].iter().rev() {
                    state.slave_readable.push_front(*byte);
                }
                state.slave_read_id = None;
            }
            TerminalEndpoint::Console => return Err(PtyError::Invalid),
        }
        self.finished = true;
        drop(state);
        self.pair.master_space_wait.wake_all();
        self.pair.slave_space_wait.wake_all();
        if copied == 0 && !complete { Err(PtyError::Invalid) } else { Ok(copied) }
    }
}

impl Drop for PtyReadReservation {
    fn drop(&mut self) {
        if self.finished { return; }
        let mut state = self.pair.state.lock();
        match self.endpoint {
            TerminalEndpoint::PtyMaster if state.master_read_id == Some(self.id) => {
                for byte in self.bytes.iter().rev() { state.master_readable.push_front(*byte); }
                state.master_read_id = None;
            }
            TerminalEndpoint::PtySlave if state.slave_read_id == Some(self.id) => {
                for byte in self.bytes.iter().rev() { state.slave_readable.push_front(*byte); }
                state.slave_read_id = None;
            }
            _ => {}
        }
    }
}

pub enum PtyPreparedRead {
    Pending,
    Eof,
    /// slave 已全部关闭；Linux 上 master 读在排空输出后返回 `EIO`。
    HungUp,
    Data(PtyReadReservation),
}

/// 创建一个锁定的 UNIX98 PTY，并返回 master 打开文件描述。
pub fn allocate_pty(accmode: u32, nonblocking: bool) -> Result<PtyEndpointHandle, PtyError> {
    let mut registry = REGISTRY.lock();
    registry.pairs.retain(|_, pair| pair.strong_count() != 0);
    let number = (0..MAX_PTYS)
        .find(|number| !registry.pairs.contains_key(number))
        .ok_or(PtyError::NoSpace)?;
    let pair = SharedTerminal::new(number);
    registry.pairs.insert(number, Arc::downgrade(&pair));
    drop(registry);
    Ok(PtyEndpointHandle::new(pair, TerminalEndpoint::PtyMaster, accmode, nonblocking))
}

pub fn open_pty_slave(number: u32, accmode: u32, nonblocking: bool)
                      -> Result<PtyEndpointHandle, PtyError> {
    let pair = REGISTRY.lock().pairs.get(&number).and_then(Weak::upgrade)
        .ok_or(PtyError::NotFound)?;
    {
        let mut state = pair.state.lock();
        if state.locked { return Err(PtyError::Locked); }
        state.slave_open_descriptions = state.slave_open_descriptions.saturating_add(1);
        state.slave_hung_up = false;
    }
    Ok(PtyEndpointHandle::new(pair, TerminalEndpoint::PtySlave, accmode, nonblocking))
}

pub fn pty_numbers() -> Vec<u32> {
    let mut registry = REGISTRY.lock();
    registry.pairs.retain(|_, pair| pair.strong_count() != 0);
    registry.pairs.keys().copied().collect()
}

pub fn terminal_by_id(id: TerminalId, endpoint: TerminalEndpoint, accmode: u32,
                      nonblocking: bool) -> Result<PtyEndpointHandle, PtyError> {
    let pair = REGISTRY.lock().pairs.values().filter_map(Weak::upgrade)
        .find(|pair| pair.id == id).ok_or(PtyError::NotFound)?;
    {
        let mut state = pair.state.lock();
        match endpoint {
            TerminalEndpoint::PtyMaster => state.master_open_descriptions += 1,
            TerminalEndpoint::PtySlave => state.slave_open_descriptions += 1,
            TerminalEndpoint::Console => return Err(PtyError::Invalid),
        }
    }
    Ok(PtyEndpointHandle::new(pair, endpoint, accmode, nonblocking))
}

pub fn terminal_for_session(sid: usize, accmode: u32, nonblocking: bool)
                            -> Result<PtyEndpointHandle, PtyError> {
    let id = REGISTRY.lock().sessions.get(&sid).copied().ok_or(PtyError::NotFound)?;
    terminal_by_id(id, TerminalEndpoint::PtySlave, accmode, nonblocking)
}

pub fn attach_session(handle: &PtyEndpointHandle, sid: usize, pgid: usize) -> Result<(), PtyError> {
    if handle.endpoint != TerminalEndpoint::PtySlave || sid == 0 { return Err(PtyError::Invalid); }
    let mut registry = REGISTRY.lock();
    if registry.sessions.get(&sid).is_some_and(|id| *id != handle.id()) {
        return Err(PtyError::Busy);
    }
    {
        let mut state = handle.pair.state.lock();
        if state.controlling_sid != 0 && state.controlling_sid != sid {
            return Err(PtyError::Busy);
        }
        state.controlling_sid = sid;
        state.foreground_pgid = pgid;
    }
    registry.sessions.insert(sid, handle.id());
    Ok(())
}

pub fn detach_session(handle: &PtyEndpointHandle, sid: usize) -> Result<(), PtyError> {
    let mut registry = REGISTRY.lock();
    let mut state = handle.pair.state.lock();
    if state.controlling_sid != sid { return Err(PtyError::Invalid); }
    state.controlling_sid = 0;
    state.foreground_pgid = 0;
    registry.sessions.remove(&sid);
    Ok(())
}

/// 会话 leader 退出时删除控制终端关联。
///
/// 该操作只清理归属元数据，不关闭仍被其它进程持有的 master/slave fd。
pub fn detach_session_by_sid(sid: usize) {
    let pair = {
        let mut registry = REGISTRY.lock();
        let Some(id) = registry.sessions.remove(&sid) else { return; };
        registry.pairs.values().filter_map(Weak::upgrade).find(|pair| pair.id == id)
    };
    if let Some(pair) = pair {
        let mut state = pair.state.lock();
        if state.controlling_sid == sid {
            state.controlling_sid = 0;
            state.foreground_pgid = 0;
        }
    }
}

pub fn take_control_events(id: TerminalId) -> Vec<TtyControlEvent> {
    let pair = REGISTRY.lock().pairs.values().filter_map(Weak::upgrade).find(|pair| pair.id == id);
    let Some(pair) = pair else { return Vec::new(); };
    let mut state = pair.state.lock();
    state.events.drain(..).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix98_lock_data_and_output_flow() {
        let master = allocate_pty(2, false).expect("allocate master");
        let number = master.number();
        assert_eq!(open_pty_slave(number, 2, false).err(), Some(PtyError::Locked));
        master.set_locked(false);
        let slave = open_pty_slave(number, 2, false).expect("open slave");

        assert_eq!(master.write(b"hello\n"), Ok(6));
        let PtyPreparedRead::Data(read) = slave.prepare_read(32, false) else {
            panic!("slave did not receive canonical line");
        };
        assert_eq!(read.bytes(), b"hello\n");
        assert_eq!(read.finish(6, true), Ok(6));

        assert_eq!(slave.write(b"ok\n"), Ok(3));
        let PtyPreparedRead::Data(read) = master.prepare_read(32, false) else {
            panic!("master did not receive slave output");
        };
        // master 同时收到输入行的 echo 和 slave 的输出。
        assert!(read.bytes().ends_with(b"ok\r\n"));
        let len = read.bytes().len();
        assert_eq!(read.finish(len, true), Ok(len));
    }

    #[test]
    fn control_character_is_queued_for_foreground_group() {
        let master = allocate_pty(2, false).expect("allocate master");
        master.set_locked(false);
        let slave = open_pty_slave(master.number(), 2, false).expect("open slave");
        slave.set_foreground_pgid(42);
        assert_eq!(master.write(&[TtyTermios::DEFAULT.cc[VINTR],
                                  TtyTermios::DEFAULT.cc[VQUIT],
                                  TtyTermios::DEFAULT.cc[VSUSP]]), Ok(3));
        assert_eq!(take_control_events(master.id()),
                   alloc::vec![TtyControlEvent { process_group: 42, signal: SIGINT },
                               TtyControlEvent { process_group: 42, signal: SIGQUIT },
                               TtyControlEvent { process_group: 42, signal: SIGTSTP }]);
    }
}
