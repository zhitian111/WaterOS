//! 平台提供的调度 tick 频率。
//!
//! 引导期可先由 DTB 覆盖频率；否则使用当前 profile 的安全回退值。

use core::sync::atomic::{AtomicU64, Ordering};

pub use crate::active_impl::time::PlatformTimeImpl;
pub use api_v0::time::{PlatformTime, PlatformTimeError, PlatformTimeResult};

/// 引导期探测到的 timebase 频率缓存；0 表示尚未由 DTB 覆盖。
///
/// TIME_CONTRACT: 一旦 timer 开始以该频率换算 deadline，就不得并发修改它；当前 API
/// 只允许 boot 阶段写入，运行期只读。
static TIMEBASE_HZ_CACHE: AtomicU64 = AtomicU64::new(0);

/// 写入 DTB 等引导信息探测到的 timebase 频率。
///
/// BOOT_CONTRACT: 必须在任何 `timer::now_duration` 或 `set_timer_after` 调用之前完成；
/// 传入 0 会被拒绝，避免除零或将所有 timeout 编程为立即到期。
#[inline]
pub fn set_frequency_hz(hz: u64) -> PlatformTimeResult<()> {
    if hz == 0 {
        return Err(PlatformTimeError::InvalidFrequency);
    }
    TIMEBASE_HZ_CACHE.store(hz, Ordering::Release);
    Ok(())
}

/// 读取已探测频率，或回退到当前 platform profile 的默认值。
///
/// 回退值只用于尚未解析 DTB 的早期启动；profile 的常量应与该 machine 的硬件 tick
/// 源同刻度，而不是 wall-clock 或 scheduler 人工 tick。
#[inline]
pub fn frequency_hz() -> PlatformTimeResult<u64> {
    let cached = TIMEBASE_HZ_CACHE.load(Ordering::Acquire);
    if cached != 0 {
        return Ok(cached);
    }
    PlatformTimeImpl::get_time_frequency_hz()
}
