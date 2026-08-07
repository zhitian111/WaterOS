//! 基于系统控制台的 WaterOS TTY 行规程实现。

#![no_std]

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use api_v0::*;
use spin::Mutex;
use waitqueue::{TaskWaitResult, WaitQueue};

/// 一次独占的 TTY 读取预约。
///
/// 字节已经暂时从全局输入队列移出，必须交给 [`finish_read`] 提交或回滚。
pub struct TtyReadReservation {
    id: u64,
    bytes: Vec<u8>,
}

impl TtyReadReservation {
    /// 返回本次预约允许复制到用户空间的字节。
    pub fn bytes(&self) -> &[u8] { &self.bytes }
}

/// [`prepare_read`] 和 [`prepare_partial_read`] 的准备结果。
pub enum TtyPreparedRead {
    /// 当前尚不满足读取条件，调用方应等待或按非阻塞语义返回。
    Pending,
    /// 输入端已经关闭，或者 canonical EOF 已经就绪。
    Eof,
    /// 已经取得独占读取预约。
    Data(TtyReadReservation),
}

/// 单个输入字节最多产生的短回显序列。
#[derive(Clone, Copy)]
struct EchoBytes {
    bytes: [u8; 8],
    len: usize,
}

impl EchoBytes {
    const NONE: Self = Self { bytes: [0; 8], len: 0 };
    fn one(byte: u8) -> Self {
        let mut echo = Self::NONE;
        echo.bytes[0] = byte;
        echo.len = 1;
        echo
    }
}

/// 系统控制台的全部共享行规程状态，由 [`TTY`] 的自旋锁保护。
struct TtyState {
    /// stdin 来源策略。
    mode: ConsoleTtyMode,
    /// 当前行规程配置。
    termios: TtyTermios,
    /// 对用户态报告的窗口尺寸。
    winsize: TtyWinSize,
    /// 可以接收终端控制信号的前台进程组。
    foreground_pgid: usize,
    /// 当前拥有该控制终端的会话。
    controlling_sid: usize,
    /// 已完成行规程处理、可以交付给读取者的字节。
    readable: VecDeque<u8>,
    /// canonical 模式下尚未提交的一行。
    editing: Vec<u8>,
    /// 空行收到 VEOF 后待交付的一次 EOF。
    eof_pending: bool,
    /// 当前独占读取预约的序列号。
    active_read: Option<u64>,
    /// 下一次读取预约使用的序列号。
    next_read_id: u64,
}

impl TtyState {
    const fn new() -> Self {
        Self {
            mode: ConsoleTtyMode::Closed,
            termios: TtyTermios::DEFAULT,
            winsize: TtyWinSize::DEFAULT,
            foreground_pgid: 0,
            controlling_sid: 0,
            readable: VecDeque::new(),
            editing: Vec::new(),
            eof_pending: false,
            active_read: None,
            next_read_id: 1,
        }
    }

    fn canonical(&self) -> bool { self.termios.lflag & ICANON != 0 }
    fn echo(&self) -> bool { self.termios.lflag & ECHO != 0 }
    fn signals(&self) -> bool { self.termios.lflag & ISIG != 0 }

    fn replenish_fixture(&mut self) {
        if self.mode == ConsoleTtyMode::Fixture && self.readable.is_empty() &&
           self.active_read.is_none()
        {
            self.readable.extend(b"password\n");
        }
    }

    fn readable_now(&self) -> bool {
        if self.eof_pending || !self.readable.is_empty() {
            if self.canonical() {
                return true;
            }
            let minimum = usize::from(self.termios.cc[VMIN]);
            return (minimum == 0 && !self.readable.is_empty()) ||
                   self.readable.len() >= minimum;
        }
        !self.canonical() && self.termios.cc[VMIN] == 0 && self.termios.cc[VTIME] == 0
    }

    fn readable_for(&self, max_len: usize) -> bool {
        if self.eof_pending || self.mode == ConsoleTtyMode::Closed {
            return true;
        }
        if self.canonical() {
            return !self.readable.is_empty();
        }
        let minimum = usize::from(self.termios.cc[VMIN]).min(max_len);
        if minimum == 0 {
            !self.readable.is_empty() || self.termios.cc[VTIME] == 0
        } else {
            self.readable.len() >= minimum
        }
    }
}

/// 唯一系统控制台的共享状态。
static TTY: Mutex<TtyState> = Mutex::new(TtyState::new());
/// 懒初始化的输入等待队列；单独加锁以避免在 TTY 锁内执行唤醒。
static INPUT_WAIT: Mutex<Option<WaitQueue>> = Mutex::new(None);

fn input_wait_queue() -> WaitQueue {
    let mut wait = INPUT_WAIT.lock();
    *wait.get_or_insert_with(|| WaitQueue::new_named("console-tty-input"))
}

fn wake_input_waiters() {
    let wait = *INPUT_WAIT.lock();
    if let Some(wait) = wait {
        wait.wake_all();
    }
}

/// 设置控制台输入模式，并把 termios、会话和缓冲区恢复为初始状态。
pub fn configure(mode: ConsoleTtyMode) {
    let mut tty = TTY.lock();
    tty.mode = mode;
    tty.termios = TtyTermios::DEFAULT;
    tty.foreground_pgid = 0;
    tty.controlling_sid = 0;
    tty.readable.clear();
    tty.editing.clear();
    tty.eof_pending = mode == ConsoleTtyMode::Closed;
    tty.active_read = None;
    if mode == ConsoleTtyMode::Fixture {
        for byte in b"password\n" {
            tty.readable.push_back(*byte);
        }
    }
    drop(tty);
    wake_input_waiters();
}

/// 返回当前控制台输入模式。
pub fn mode() -> ConsoleTtyMode { TTY.lock().mode }

/// 返回当前 termios 快照。
pub fn termios() -> TtyTermios { TTY.lock().termios }

/// 更新 termios；`flush_input` 为真时同时丢弃所有未读和正在编辑的输入。
pub fn set_termios(termios: TtyTermios, flush_input: bool) {
    let mut tty = TTY.lock();
    let was_canonical = tty.canonical();
    if flush_input {
        tty.readable.clear();
        tty.editing.clear();
        tty.eof_pending = false;
    } else if was_canonical && termios.lflag & ICANON == 0 {
        let editing = core::mem::take(&mut tty.editing);
        tty.readable.extend(editing);
    }
    tty.termios = termios;
}

/// 返回终端窗口尺寸快照。
pub fn winsize() -> TtyWinSize { TTY.lock().winsize }
/// 更新终端窗口尺寸；`SIGWINCH` 由 syscall 层在锁外投递。
pub fn set_winsize(winsize: TtyWinSize) { TTY.lock().winsize = winsize; }
/// 返回当前前台进程组 ID；零表示尚未设置。
pub fn foreground_pgid() -> usize { TTY.lock().foreground_pgid }
/// 设置能够读取终端并接收控制字符信号的前台进程组。
pub fn set_foreground_pgid(pgid: usize) { TTY.lock().foreground_pgid = pgid; }
/// 返回控制会话 ID；零表示终端尚未归属任何会话。
pub fn controlling_sid() -> usize { TTY.lock().controlling_sid }
/// 设置拥有该控制终端的会话。
pub fn set_controlling_sid(sid: usize) { TTY.lock().controlling_sid = sid; }
/// 解除控制会话，同时清除前台进程组。
pub fn detach_controlling_terminal() {
    let mut tty = TTY.lock();
    tty.controlling_sid = 0;
    tty.foreground_pgid = 0;
}
/// 返回是否启用了 `TOSTOP` 后台写终端检查。
pub fn output_stops_background() -> bool { TTY.lock().termios.lflag & TOSTOP != 0 }

/// 输入一个原始 UART 字节。
///
/// 返回回显字节和可选信号事件，由调用方在释放 TTY 锁后执行输出和信号投递。
pub fn feed_input(mut byte: u8) -> (Option<TtyControlEvent>, [u8; 8], usize) {
    let mut tty = TTY.lock();
    if tty.mode != ConsoleTtyMode::Interactive {
        return (None, [0; 8], 0);
    }
    if byte == b'\r' && tty.termios.iflag & ICRNL != 0 {
        byte = b'\n';
    }

    let mut event = None;
    let mut echo = EchoBytes::NONE;
    if tty.signals() {
        let signal = if byte == tty.termios.cc[VINTR] {
            Some(SIGINT)
        } else if byte == tty.termios.cc[VQUIT] {
            Some(SIGQUIT)
        } else if byte == tty.termios.cc[VSUSP] {
            Some(SIGTSTP)
        } else {
            None
        };
        if let Some(signal) = signal {
            tty.editing.clear();
            if tty.echo() {
                let marker = match signal { SIGINT => b'C', SIGQUIT => b'\\', _ => b'Z' };
                echo.bytes[..4].copy_from_slice(&[b'^', marker, b'\r', b'\n']);
                echo.len = 4;
            }
            if tty.foreground_pgid != 0 {
                event = Some(TtyControlEvent { process_group: tty.foreground_pgid, signal });
            }
            drop(tty);
            wake_input_waiters();
            return (event, echo.bytes, echo.len);
        }
    }

    if tty.canonical() {
        if byte == tty.termios.cc[VERASE] || byte == 8 {
            if tty.editing.pop().is_some() && tty.echo() {
                echo.bytes[..3].copy_from_slice(b"\x08 \x08");
                echo.len = 3;
            }
        } else if byte == tty.termios.cc[VKILL] {
            tty.editing.clear();
            if tty.echo() {
                echo.bytes[..3].copy_from_slice(b"^U\n");
                echo.len = 3;
            }
        } else if byte == tty.termios.cc[VEOF] {
            if tty.editing.is_empty() {
                tty.eof_pending = true;
            } else {
                let editing = core::mem::take(&mut tty.editing);
                tty.readable.extend(editing);
            }
        } else if byte == b'\n' {
            let editing = core::mem::take(&mut tty.editing);
            tty.readable.extend(editing);
            tty.readable.push_back(b'\n');
            if tty.echo() {
                echo.bytes[..2].copy_from_slice(b"\r\n");
                echo.len = 2;
            }
        } else {
            tty.editing.push(byte);
            if tty.echo() {
                echo = EchoBytes::one(byte);
            }
        }
    } else {
        tty.readable.push_back(byte);
        if tty.echo() {
            echo = EchoBytes::one(byte);
        }
    }
    drop(tty);
    wake_input_waiters();
    (event, echo.bytes, echo.len)
}

/// 按 canonical 或 `VMIN/VTIME` 即时条件预约最多 `max_len` 个输入字节。
pub fn prepare_read(max_len: usize) -> TtyPreparedRead {
    let mut tty = TTY.lock();
    tty.replenish_fixture();
    if tty.mode == ConsoleTtyMode::Closed {
        return TtyPreparedRead::Eof;
    }
    if tty.active_read.is_some() || !tty.readable_for(max_len) {
        return TtyPreparedRead::Pending;
    }
    if tty.eof_pending && tty.readable.is_empty() {
        tty.eof_pending = false;
        return TtyPreparedRead::Eof;
    }
    let len = max_len.min(tty.readable.len());
    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(len).is_err() {
        return TtyPreparedRead::Pending;
    }
    for _ in 0..len {
        if let Some(byte) = tty.readable.pop_front() {
            bytes.push(byte);
        }
    }
    let id = tty.next_read_id;
    tty.next_read_id = tty.next_read_id.wrapping_add(1);
    tty.active_read = Some(id);
    TtyPreparedRead::Data(TtyReadReservation { id, bytes })
}

/// 即使缓冲字节少于 `VMIN`，也预约当前已有的 raw 输入。
///
/// 仅用于非 canonical 模式的字节间计时器超时路径。
pub fn prepare_partial_read(max_len: usize) -> TtyPreparedRead {
    let mut tty = TTY.lock();
    if tty.active_read.is_some() || tty.readable.is_empty() {
        return if tty.mode == ConsoleTtyMode::Closed {
            TtyPreparedRead::Eof
        } else {
            TtyPreparedRead::Pending
        };
    }
    let len = max_len.min(tty.readable.len());
    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(len).is_err() {
        return TtyPreparedRead::Pending;
    }
    for _ in 0..len {
        bytes.push(tty.readable.pop_front().expect("TTY length checked"));
    }
    let id = tty.next_read_id;
    tty.next_read_id = tty.next_read_id.wrapping_add(1);
    tty.active_read = Some(id);
    TtyPreparedRead::Data(TtyReadReservation { id, bytes })
}

/// 完成读取预约。
///
/// `copied` 字节被正式消费；剩余字节按原顺序放回队首。`complete=false` 且没有
/// 复制任何字节时返回错误，用于向上层表达用户复制失败。
pub fn finish_read(reservation: TtyReadReservation,
                   copied: usize,
                   complete: bool)
                   -> Result<usize, ()> {
    let mut tty = TTY.lock();
    if tty.active_read != Some(reservation.id) || copied > reservation.bytes.len() {
        return Err(());
    }
    for byte in reservation.bytes[copied..].iter().rev() {
        tty.readable.push_front(*byte);
    }
    tty.active_read = None;
    if copied == 0 && !complete { Err(()) } else { Ok(copied) }
}

/// 按当前行规程判断 `poll/select` 是否应报告 stdin 可读。
pub fn poll_readable() -> bool { TTY.lock().readable_now() }

/// 阻塞当前任务，直到处理后的输入满足读取条件，或者等待被信号中断。
///
/// 条件检查与加入任务等待队列原子衔接，避免 UART 字节恰好在检查与睡眠之间到达而
/// 造成丢失唤醒。
pub fn wait_for_input(max_len: usize) -> TaskWaitResult {
    // POSIX 实际等待阈值是 VMIN 与调用方缓冲区长度中的较小值。poll 没有缓冲区
    // 长度，因此仍使用 `readable_now`；阻塞 read 的请求长度小于 VMIN 时则不能
    // 永远等待完整 VMIN。
    input_wait_queue().wait_current_while(|| !TTY.lock().readable_for(max_len))
}

/// 等待处理后输入的长度发生变化，或者等待指定 tick 数后超时。
///
/// 通过等待长度变化实现 POSIX 非 canonical 字节间计时器：每个新字节都会重新开始
/// `VTIME` 计时。
pub fn wait_for_input_change_for_ticks(previous_len: usize,
                                       timeout_ticks: u64)
                                       -> TaskWaitResult {
    input_wait_queue().wait_current_while_for_ticks(timeout_ticks,
                                                    || TTY.lock().readable.len() == previous_len)
}

/// 不设超时地等待至少一个新的处理后字节。
///
/// 这是 `VMIN>0,VTIME>0` 语义中“收到第一个字节后才启动 VTIME”的前半部分。
pub fn wait_for_input_change(previous_len: usize) -> TaskWaitResult {
    input_wait_queue().wait_current_while(|| TTY.lock().readable.len() == previous_len)
}

/// 返回一次读取尝试使用的 `(canonical, VMIN, VTIME)` 快照。
pub fn read_settings() -> (bool, usize, u64) {
    let tty = TTY.lock();
    (tty.canonical(),
     usize::from(tty.termios.cc[VMIN]),
     u64::from(tty.termios.cc[VTIME]))
}

/// 当前可立即交付给 `read(2)` 的处理后字节数。
pub fn readable_len() -> usize { TTY.lock().readable.len() }

/// 按 `OPOST/ONLCR` 转换用户态输出；返回值是应直接写入 UART 的线缆字节。
pub fn transform_output(input: &[u8]) -> Vec<u8> {
    let termios = TTY.lock().termios;
    if termios.oflag & OPOST == 0 || termios.oflag & ONLCR == 0 || !input.contains(&b'\n') {
        return input.to_vec();
    }
    let mut output = Vec::with_capacity(input.len().saturating_add(input.len() / 8 + 1));
    for byte in input {
        if *byte == b'\n' {
            output.push(b'\r');
        }
        output.push(*byte);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn canonical_editing_and_eof() {
        let _test = TEST_LOCK.lock();
        configure(ConsoleTtyMode::Interactive);
        feed_input(b'a');
        feed_input(127);
        feed_input(b'b');
        feed_input(b'\n');
        let TtyPreparedRead::Data(read) = prepare_read(8) else { panic!("not readable") };
        assert_eq!(read.bytes(), b"b\n");
        assert_eq!(finish_read(read, 2, true), Ok(2));
        feed_input(TtyTermios::DEFAULT.cc[VEOF]);
        assert!(matches!(prepare_read(8), TtyPreparedRead::Eof));
    }

    #[test]
    fn raw_mode_is_immediately_readable() {
        let _test = TEST_LOCK.lock();
        configure(ConsoleTtyMode::Interactive);
        let mut termios = termios();
        termios.lflag &= !ICANON;
        set_termios(termios, false);
        feed_input(0x1b);
        let TtyPreparedRead::Data(read) = prepare_read(1) else { panic!("not readable") };
        assert_eq!(read.bytes(), &[0x1b]);
        assert_eq!(finish_read(read, 1, true), Ok(1));
    }

    #[test]
    fn raw_vmin_and_vtime_control_readiness() {
        let _test = TEST_LOCK.lock();
        configure(ConsoleTtyMode::Interactive);
        let mut settings = termios();
        settings.lflag &= !ICANON;
        settings.cc[VMIN] = 2;
        settings.cc[VTIME] = 0;
        set_termios(settings, false);
        feed_input(b'a');
        assert!(!poll_readable());
        assert!(matches!(prepare_read(8), TtyPreparedRead::Pending));
        feed_input(b'b');
        assert!(poll_readable());
        let TtyPreparedRead::Data(read) = prepare_read(8) else { panic!("VMIN not honored") };
        assert_eq!(read.bytes(), b"ab");
        assert_eq!(finish_read(read, 2, true), Ok(2));

        settings.cc[VMIN] = 0;
        settings.cc[VTIME] = 0;
        set_termios(settings, true);
        assert!(poll_readable());
        let TtyPreparedRead::Data(read) = prepare_read(8) else { panic!("zero read pending") };
        assert!(read.bytes().is_empty());
        assert_eq!(finish_read(read, 0, true), Ok(0));

        settings.cc[VTIME] = 1;
        set_termios(settings, true);
        assert!(!poll_readable());
        assert!(matches!(prepare_read(8), TtyPreparedRead::Pending));
    }

    #[test]
    fn raw_read_caps_vmin_to_caller_buffer() {
        let _test = TEST_LOCK.lock();
        configure(ConsoleTtyMode::Interactive);
        let mut settings = termios();
        settings.lflag &= !ICANON;
        settings.cc[VMIN] = 8;
        settings.cc[VTIME] = 0;
        set_termios(settings, false);
        feed_input(b'x');
        let TtyPreparedRead::Data(read) = prepare_read(1) else {
            panic!("one-byte read incorrectly waited for the full VMIN");
        };
        assert_eq!(read.bytes(), b"x");
        assert_eq!(finish_read(read, 1, true), Ok(1));
    }

    #[test]
    fn control_character_targets_foreground_group() {
        let _test = TEST_LOCK.lock();
        configure(ConsoleTtyMode::Interactive);
        set_foreground_pgid(42);
        let (event, _, _) = feed_input(TtyTermios::DEFAULT.cc[VINTR]);
        assert_eq!(event,
                   Some(TtyControlEvent { process_group: 42, signal: SIGINT }));
    }
}
