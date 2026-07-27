//! Futex 等待结果。

/// 带条件与可选超时的 futex 等待结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FutexWaitOutcome {
    /// 正常被唤醒，或阻塞前复查发现条件已经不成立。
    Woken,
    /// 超时到期且等待条件仍成立。
    TimedOut,
    /// 等待被信号中断。
    Interrupted,
}
