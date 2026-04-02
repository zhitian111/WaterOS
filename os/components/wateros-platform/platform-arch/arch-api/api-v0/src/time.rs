use core::result::Result;

/// 架构层时间错误：只描述“原语不可用/不支持”的最小语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchTimeError {
    Unsupported,
    Unavailable,
}

/// 架构层时间戳（硬件计数器原始 tick）。
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ArchTimeTick(pub u64);

/// 架构层时间频率（tick 每秒）。不同平台可能未知，所以是可选能力。
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ArchTimeFrequency(pub u64);

pub type ArchTimeResult<T> = Result<T, ArchTimeError>;

/// 架构时间原语接口：
/// - 只读时间计数器
/// - 可选读频率
///
/// 不包含 set_timer。定时器编程由 firmware/platform 层组合实现。
pub trait ArchTime {
    /// 读取当前硬件时间计数器（单调递增的原始 tick）。
    fn read_time_tick() -> ArchTimeResult<ArchTimeTick>;

    /// 读取硬件时间频率（tick/s），默认不支持。
    #[inline]
    fn read_time_frequency() -> ArchTimeResult<ArchTimeFrequency> {
        Err(ArchTimeError::Unsupported)
    }
}
