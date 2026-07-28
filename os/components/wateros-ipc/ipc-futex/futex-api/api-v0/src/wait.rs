//! Futex 等待结果。

/// 带条件与可选超时的 futex 等待结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FutexWaitOutcome {
    /// 正常被 futex wake 唤醒。
    Woken,
    /// 阻塞前的原子化复查发现等待条件已不成立。
    ConditionChanged,
    /// 超时到期且等待条件仍成立。
    TimedOut,
    /// 等待被信号中断。
    Interrupted,
}
