use core::result::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformTimeError {
    Unsupported,
    Unavailable,
    InvalidFrequency,
}

pub type PlatformTimeResult<T> = Result<T, PlatformTimeError>;

/// 平台层时间能力：
/// 频率属于平台定义（可来自 DTB/固件/板级常量），不放在 arch 层硬编码。
pub trait PlatformTime {
    #[inline]
    fn time_frequency_hz() -> PlatformTimeResult<u64> {
        Err(PlatformTimeError::Unsupported)
    }
}

