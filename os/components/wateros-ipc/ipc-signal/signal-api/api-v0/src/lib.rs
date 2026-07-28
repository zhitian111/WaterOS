#![no_std]
//! 信号 API v0：Linux 风格信号编号、掩码与 `rt_sigaction` 状态类型。
//!
//! 本 crate 只定义跨实现稳定的类型与常量；状态机由具体实现 crate 提供。

/// 支持的最大信号编号（不含 0）。
pub const NSIG : usize = 64;
/// 默认处理（`SIG_DFL`）。
pub const SIG_DFL : usize = 0;
/// 忽略信号（`SIG_IGN`）。
pub const SIG_IGN : usize = 1;
/// `sigprocmask`：阻塞集合中加入 `set`。
pub const SIG_BLOCK : usize = 0;
/// `sigprocmask`：从阻塞集合移除 `set`。
pub const SIG_UNBLOCK : usize = 1;
/// `sigprocmask`：直接替换阻塞集合。
pub const SIG_SETMASK : usize = 2;

pub const SIGHUP : usize = 1;
pub const SIGINT : usize = 2;
pub const SIGILL : usize = 4;
pub const SIGBUS : usize = 7;
pub const SIGFPE : usize = 8;
pub const SIGKILL : usize = 9;
pub const SIGUSR1 : usize = 10;
pub const SIGSEGV : usize = 11;
pub const SIGUSR2 : usize = 12;
pub const SIGALRM : usize = 14;
pub const SIGTERM : usize = 15;
pub const SIGPIPE : usize = 13;
pub const SIGCHLD : usize = 17;
pub const SIGCONT : usize = 18;
pub const SIGSTOP : usize = 19;
pub const SIGTSTP : usize = 20;
pub const SIGTTIN : usize = 21;
pub const SIGTTOU : usize = 22;
pub const SIGURG : usize = 23;
pub const SIGVTALRM : usize = 26;
pub const SIGPROF : usize = 27;
pub const SIGWINCH : usize = 28;

pub const SA_NOCLDSTOP : usize = 0x0000_0001;
pub const SA_NOCLDWAIT : usize = 0x0000_0002;
pub const SA_SIGINFO : usize = 0x0000_0004;
pub const SA_ONSTACK : usize = 0x0800_0000;
pub const SA_RESTART : usize = 0x1000_0000;
pub const SA_NODEFER : usize = 0x4000_0000;
pub const SA_RESETHAND : usize = 0x8000_0000;
pub const SA_RESTORER : usize = 0x0400_0000;

/// `setitimer` / `getitimer` 定时器种类。
pub const ITIMER_REAL : usize = 0;
pub const ITIMER_VIRTUAL : usize = 1;
pub const ITIMER_PROF : usize = 2;

/// 信号子系统错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalError {
    /// 信号编号非法或不可更改 disposition。
    InvalidSignal,
    /// `sigprocmask` / `sigsuspend` 的 `how` 非法。
    InvalidHow,
    /// 任务 id 未注册。
    NoSuchTask,
    /// 进程 id 未注册。
    NoSuchProcess,
    /// `itimer` 种类非法。
    InvalidTimer,
    /// 当前线程正在备用信号栈上执行，不能替换该栈。
    AlternateStackActive,
    /// POSIX timer id 不属于目标进程。
    NoSuchTimer,
}

/// 信号操作结果。
pub type SignalResult<T> = Result<T, SignalError>;

/// 64 位信号掩码（每位对应一个信号号）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SignalSet(u64);

impl SignalSet {
    /// 空掩码。
    pub const fn empty() -> Self { Self(0) }

    /// 从原始位图构造。
    pub const fn from_bits(bits : u64) -> Self { Self(bits) }

    /// 返回原始位图。
    pub const fn bits(self) -> u64 { self.0 }

    /// 掩码是否包含信号 `sig`。
    pub const fn contains(self, sig : usize) -> bool {
        match signal_bit(sig) {
            Some(bit) => self.0 & bit != 0,
            None => false,
        }
    }

    /// 将信号加入掩码。
    pub fn insert(&mut self, sig : usize) {
        if let Some(bit) = signal_bit(sig) {
            self.0 |= bit;
        }
    }

    /// 将信号从掩码移除。
    pub fn remove(&mut self, sig : usize) {
        if let Some(bit) = signal_bit(sig) {
            self.0 &= !bit;
        }
    }

    /// 并集。
    pub fn union(self, other : Self) -> Self { Self(self.0 | other.0) }

    /// 差集（`self` 中有而 `other` 中没有的位）。
    pub fn difference(self, other : Self) -> Self { Self(self.0 & !other.0) }

    /// 交集。
    pub fn intersection(self, other : Self) -> Self { Self(self.0 & other.0) }

    /// 是否为空掩码。
    pub const fn is_empty(self) -> bool { self.0 == 0 }

    /// 返回最低位已置位信号号（1-based）。
    pub fn first_signal(self) -> Option<usize> {
        if self.0 == 0 {
            return None;
        }
        Some(self.0
                 .trailing_zeros() as usize +
             1)
    }
}

/// `struct sigaction` 的 IPC 层视图（与 Linux `rt_sigaction` 布局对齐）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct SignalAction {
    /// 处理函数地址；`SIG_DFL` / `SIG_IGN` 为特殊值。
    pub handler : usize,
    /// `SA_*` 标志位。
    pub flags : usize,
    /// `SA_RESTORER` 恢复桩地址。
    pub restorer : usize,
    /// 进入处理函数时临时阻塞的信号集。
    pub mask : SignalSet,
}

impl SignalAction {
    /// 默认 disposition（内核默认语义）。
    pub const fn default_action() -> Self {
        Self { handler : SIG_DFL,
               flags : 0,
               restorer : 0,
               mask : SignalSet::empty() }
    }

    /// 忽略 disposition。
    pub const fn ignore() -> Self {
        Self { handler : SIG_IGN,
               flags : 0,
               restorer : 0,
               mask : SignalSet::empty() }
    }

    /// 是否为默认处理。
    pub const fn is_default(self) -> bool { self.handler == SIG_DFL }

    /// 是否忽略。
    pub const fn is_ignore(self) -> bool { self.handler == SIG_IGN }

    /// 是否安装了用户态处理函数。
    pub const fn has_user_handler(self) -> bool { self.handler > SIG_IGN }
}

/// 信号生成阶段的路由结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalDelivery {
    /// 被忽略或默认忽略。
    Ignored,
    /// 应进入 pending 集等待交付。
    Pending,
    /// `SIGSTOP` 的不可屏蔽进程停止副作用。
    Stop,
    /// 应继续目标（`SIGCONT`）。
    Continue,
}

/// `setitimer` 规格：间隔与当前剩余时间（纳秒）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IntervalTimerSpec {
    /// 重复间隔。
    pub interval_ns : u128,
    /// 距下次到期的剩余时间；0 表示禁用。
    pub value_ns : u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PosixTimerClock {
    Realtime,
    Monotonic,
}

/// 待交付给陷阱处理器的信号帧信息。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingSignal {
    /// 信号编号。
    pub signal : usize,
    /// 交付时的 disposition 快照。
    pub action : SignalAction,
    /// 进入处理函数前应恢复的线程掩码。
    pub previous_mask : SignalSet,
}

/// 目标线程在返回用户态前取出的信号效果。
///
/// 信号生成阶段只负责写入 pending；disposition 必须到目标线程的安全点才判断，
/// 因为 pending 期间线程 mask 和进程 sigaction 都可能改变。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalEffect {
    Handler(PendingSignal),
    Terminate { signal : usize },
    Stop { signal : usize },
    Continue { signal : usize },
}

/// 线程备用信号栈状态。
///
/// 地址与大小来自 `sigaltstack(2)`；`active_frames` 由内核在建立/恢复信号帧时维护。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AlternateSignalStack {
    pub sp : usize,
    pub size : usize,
    pub active_frames : usize,
}

impl AlternateSignalStack {
    pub const fn is_enabled(self) -> bool { self.size != 0 }

    pub fn contains(self, sp : usize) -> bool {
        self.is_enabled() &&
        sp >= self.sp &&
        self.sp
            .checked_add(self.size)
            .is_some_and(|end| sp < end)
    }
}

/// `kill` / `tkill` 等路径的投递摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignalDispatch {
    /// 投递分类。
    pub delivery : SignalDelivery,
    /// 需要唤醒的任务 id（pending 路径）。
    pub target_task_id : Option<usize>,
}

impl SignalDispatch {
    pub const fn ignored() -> Self {
        Self { delivery : SignalDelivery::Ignored,
               target_task_id : None }
    }

    pub const fn pending(target_task_id : Option<usize>) -> Self {
        Self { delivery : SignalDelivery::Pending,
               target_task_id }
    }

    pub const fn stop(target_task_id : Option<usize>) -> Self {
        Self { delivery : SignalDelivery::Stop,
               target_task_id }
    }

    pub const fn continued(target_task_id : Option<usize>) -> Self {
        Self { delivery : SignalDelivery::Continue,
               target_task_id }
    }
}

/// 信号编号是否在 `1..=NSIG` 范围内。
pub const fn valid_signal(sig : usize) -> bool { sig > 0 && sig <= NSIG }

/// `itimer` 种类是否合法。
pub const fn valid_itimer(which : usize) -> bool {
    matches!(which,
             ITIMER_REAL | ITIMER_VIRTUAL | ITIMER_PROF)
}

/// 信号是否不可被阻塞或更改 disposition。
pub const fn immutable_signal(sig : usize) -> bool { sig == SIGKILL || sig == SIGSTOP }

/// 将信号号转为掩码位；非法编号返回 `None`。
pub const fn signal_bit(sig : usize) -> Option<u64> {
    if valid_signal(sig) {
        Some(1u64 << (sig - 1))
    } else {
        None
    }
}

/// 默认 disposition 下应忽略的信号。
pub const fn default_ignored(sig : usize) -> bool {
    sig == SIGCHLD || sig == SIGURG || sig == SIGWINCH
}

/// 默认 disposition 下应停止进程的信号。
pub const fn default_stops(sig : usize) -> bool {
    matches!(sig, SIGSTOP | SIGTSTP | SIGTTIN | SIGTTOU)
}

/// 默认 disposition 下应终止进程的信号。
pub const fn default_terminates(sig : usize) -> bool {
    valid_signal(sig) && !default_ignored(sig) && !default_stops(sig) && sig != SIGCONT
}
