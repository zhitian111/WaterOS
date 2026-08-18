//! 墙上时钟（`CLOCK_REALTIME`）与单调时钟的组合语义。

use core::sync::atomic::{AtomicI64, Ordering};

/// `CLOCK_REALTIME - CLOCK_MONOTONIC` 的软件偏移。
///
/// TIME_CONTRACT: 单调时钟绝不回退；设置时间只更新该偏移。`Relaxed` 足够，因为
/// 偏移不保护其他数据，调用者只需要读到某一次完整的 `i64` 值。
static REALTIME_OFFSET_NS: AtomicI64 = AtomicI64::new(0);

/// 当前单调时钟纳秒（与 [`crate::timer::now_duration`] 同源）。
///
/// 返回错误表示底层 tick 或频率尚不可用，而不是“时间为 0”。
pub fn monotonic_ns() -> Result<u128, ()> {
    let duration = crate::timer::now_duration().map_err(|_| ())?;
    Ok(duration.as_nanos())
}

/// 当前 `CLOCK_REALTIME` 纳秒；若设置为早于 epoch 则钳制为 0。
pub fn realtime_ns() -> Result<u128, ()> {
    let mono = monotonic_ns()?;
    let offset = REALTIME_OFFSET_NS.load(Ordering::Relaxed) as i128;
    Ok(((mono as i128) + offset).max(0) as u128)
}

/// 将 `CLOCK_REALTIME` 设为 `target_ns`（相对单调时钟偏移）。
///
/// 该操作不访问 RTC 硬件，也不持久化；重启后需由更高层重新设置。
pub fn set_realtime_ns(target_ns: u128) -> Result<(), ()> {
    let mono = monotonic_ns()?;
    let offset = (target_ns as i128) - (mono as i128);
    let offset = i64::try_from(offset).unwrap_or(i64::MAX);
    REALTIME_OFFSET_NS.store(offset, Ordering::Relaxed);
    Ok(())
}

/// 将 UTC 纳秒转为 Linux `struct rtc_time` 字段（`tm_year` 为自 1900 起算）。
#[derive(Clone, Copy, Debug, Default)]
pub struct RtcTimeFields {
    /// 秒（0–59）。
    pub tm_sec: i32,
    /// 分（0–59）。
    pub tm_min: i32,
    /// 时（0–23）。
    pub tm_hour: i32,
    /// 日（1–31）。
    pub tm_mday: i32,
    /// 月（0–11）。
    pub tm_mon: i32,
    /// 年（自 1900 起算，如 2026 年为 126）。
    pub tm_year: i32,
    /// 星期（0=周日）。
    pub tm_wday: i32,
    /// 年内第几天（本实现未填，恒为 0）。
    pub tm_yday: i32,
    /// 夏令时标志（本实现未用，恒为 0）。
    pub tm_isdst: i32,
}

/// 将 UTC 纳秒时间戳拆成 Linux `struct rtc_time` 字段。
pub fn ns_to_rtc_time(ns: u128) -> RtcTimeFields {
    let total_sec = (ns / 1_000_000_000) as i64;
    let sec_of_day = total_sec.rem_euclid(86_400);
    let days = total_sec.div_euclid(86_400);

    let tm_sec = (sec_of_day % 60) as i32;
    let tm_min = ((sec_of_day / 60) % 60) as i32;
    let tm_hour = (sec_of_day / 3600) as i32;

    // 1970-01-01 为 Unix epoch；rtc_time.tm_year 为自 1900 起。
    let mut y = 1970i64;
    let mut remaining_days = days;
    loop {
        let days_in_year = if is_leap_year(y) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        y += 1;
    }

    let mut m = 0usize;
    let days_in_months = if is_leap_year(y) {
        [
            31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
        ]
    } else {
        [
            31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
        ]
    };
    while m < 12 && remaining_days >= days_in_months[m] as i64 {
        remaining_days -= days_in_months[m] as i64;
        m += 1;
    }

    let tm_wday = ((days + 4) % 7) as i32; // 1970-01-01 是周四，wday=4

    RtcTimeFields {
        tm_sec,
        tm_min,
        tm_hour,
        tm_mday: (remaining_days + 1) as i32,
        tm_mon: m as i32,
        tm_year: (y - 1900) as i32,
        tm_wday,
        tm_yday: 0,
        tm_isdst: 0,
    }
}

/// 将 Linux `struct rtc_time` 字段合成 UTC 纳秒时间戳。
pub fn rtc_time_to_ns(fields: &RtcTimeFields) -> Result<u128, ()> {
    // 这里只拒绝负值、非法月份和 Unix epoch 之前的年份；日期上限由现有
    // `days_since_epoch` 算法按月份累加，调用方应传入真实日历日期。
    if fields.tm_sec < 0
        || fields.tm_min < 0
        || fields.tm_hour < 0
        || fields.tm_mday < 1
        || fields.tm_mon < 0
        || fields.tm_mon > 11
        || fields.tm_year < 70
    {
        return Err(());
    }
    let y = fields.tm_year as i64 + 1900;
    let days = days_since_epoch(
        y,
        fields.tm_mon as usize,
        fields.tm_mday as i32,
    );
    let sec = fields.tm_hour as u128 * 3600 + fields.tm_min as u128 * 60 + fields.tm_sec as u128;
    Ok(days * 86_400 * 1_000_000_000 + sec * 1_000_000_000)
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

fn days_since_epoch(year: i64, month: usize, mday: i32) -> u128 {
    let mut days: u128 = 0;
    for y in 1970..year {
        days += if is_leap_year(y) { 366 } else { 365 };
    }
    let mdays = if is_leap_year(year) {
        [
            31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
        ]
    } else {
        [
            31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
        ]
    };
    for m in 0..month {
        days += mdays[m] as u128;
    }
    days + (mday - 1) as u128
}
