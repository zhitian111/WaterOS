//! 平台 timer 组合层实现。

use core::time::Duration;

    pub use api_v0::time::PlatformTimeError;
    pub use api_v0::timer::{
        PlatformDeadlineTimerError, PlatformDeadlineTimerResult, PlatformTimerDeadline,
    };
    pub use arch::time::{ArchTimeError, ArchTimeFrequency, ArchTimeTick};

    /// 组合定时器路径上各层失败的归并类型。
    #[derive(Debug)]
    pub enum PlatformTimerError {
        Arch(ArchTimeError),
        Platform(PlatformTimeError),
        DeadlineTimer(PlatformDeadlineTimerError),
        NoFrequency,
        Overflow,
    }

    /// [`PlatformTimerError`] 上的 `Result` 别名。
    pub type PlatformTimerResult<T> = core::result::Result<T, PlatformTimerError>;

    /// 读取 arch 单调计数器的原始 tick，不进行频率换算。
    ///
    /// TIME_CONTRACT: 返回值只在同一 arch tick 源内可比较，不能直接与 scheduler
    /// 的软件 tick、wall-clock 纳秒或其他 CPU 的不同计数源混用。
    #[inline]
    pub fn now_tick() -> PlatformTimerResult<ArchTimeTick> {
        arch::time::read_time_tick().map_err(PlatformTimerError::Arch)
    }

    /// 取得用于将 arch tick 换算为时间的频率。
    ///
    /// 频率来源由 [`crate::time`] 管理；错误意味着当前阶段尚未配置可信 timebase。
    #[inline]
    pub fn tick_hz() -> PlatformTimerResult<ArchTimeFrequency> {
        let hz = crate::time::frequency_hz().map_err(PlatformTimerError::Platform)?;
        Ok(ArchTimeFrequency(hz))
    }

    /// 将绝对 arch tick deadline 交给当前 platform profile。
    ///
    /// PLATFORM_BOUNDARY: 此处不写 CSR；RISC-V 可能经 SBI，其他 profile 可以经
    /// machine timer/MMIO。调用方必须确保 deadline 与 [`now_tick`] 同一刻度。
    #[inline]
    pub fn set_timer_deadline_tick(deadline_tick: ArchTimeTick) -> PlatformTimerResult<()> {
        crate::active_impl::timer::set_timer(PlatformTimerDeadline(deadline_tick.0))
            .map_err(PlatformTimerError::DeadlineTimer)
    }

    /// 将 duration 向上取整为 tick，保证请求不会因截断而提前触发。
    #[inline]
    fn duration_to_ticks(d: Duration, hz: u64) -> PlatformTimerResult<u64> {
        // 向上取整，避免过早触发（例如 1ns 在低频下被截断成 0 tick）。
        let nanos = d.as_nanos();
        let ticks = nanos
            .checked_mul(hz as u128)
            .ok_or(PlatformTimerError::Overflow)?
            .checked_add(1_000_000_000u128 - 1)
            .ok_or(PlatformTimerError::Overflow)?
            / 1_000_000_000u128;
        u64::try_from(ticks).map_err(|_| PlatformTimerError::Overflow)
    }

    /// 将原始 tick 换算为 duration；仅用于观测，纳秒精度会按整数除法向下截断。
    #[inline]
    fn ticks_to_duration(ticks: u64, hz: u64) -> PlatformTimerResult<Duration> {
        if hz == 0 {
            return Err(PlatformTimerError::NoFrequency);
        }
        let nanos = (ticks as u128)
            .checked_mul(1_000_000_000u128)
            .ok_or(PlatformTimerError::Overflow)?
            / (hz as u128);
        let nanos_u64 = u64::try_from(nanos).map_err(|_| PlatformTimerError::Overflow)?;
        Ok(Duration::from_nanos(nanos_u64))
    }

    /// 返回当前单调时间；不会推进 scheduler 时间，也不会编程下一次中断。
    #[inline]
    pub fn now_duration() -> PlatformTimerResult<Duration> {
        let tick = now_tick()?.0;
        let hz = tick_hz()?.0;
        ticks_to_duration(tick, hz)
    }

    /// 在当前 CPU 上编程“至少经过 `d` 后”触发的下一次 timer interrupt。
    ///
    /// TIME_CONTRACT: 这是本地硬件 deadline；全局 sleep/wait timeout 是否由该 CPU
    /// 推进由 `wateros-task` 决定，AP 不得借此重复推进 BSP 的全局 timekeeper。
    #[inline]
    pub fn set_timer_after(d: Duration) -> PlatformTimerResult<()> {
        let now = now_tick()?.0;
        let hz = tick_hz()?.0;
        if hz == 0 {
            return Err(PlatformTimerError::NoFrequency);
        }
        let delta = duration_to_ticks(d, hz)?;
        let deadline = now
            .checked_add(delta)
            .ok_or(PlatformTimerError::Overflow)?;
        log::debug!(
            "now is :{:?}, and will set to :{:?}",
            now,
            deadline
        );
        set_timer_deadline_tick(ArchTimeTick(deadline))
    }

    /// [`set_timer_after`] 的毫秒便捷封装。
    #[inline]
    pub fn set_timer_after_ms(ms: u64) -> PlatformTimerResult<()> {
        set_timer_after(Duration::from_millis(ms))
    }

    /// [`set_timer_after`] 的秒便捷封装。
    #[inline]
    pub fn set_timer_after_s(s: u64) -> PlatformTimerResult<()> {
        set_timer_after(Duration::from_secs(s))
    }
