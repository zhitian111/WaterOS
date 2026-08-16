//! Loongson 2K1000LA RTC 读取与墙上时钟初始化。

use api_v0::{DriverError, DriverResult};

#[cfg(target_arch = "loongarch64")]
use common::dtb::{compatible_list, first_mmio_region, read_fdt};

#[cfg(target_arch = "loongarch64")]
const RTC_COMPATIBLES : &[&str] = &["loongson,ls7a-rtc",
                                    "loongson,ls2k1000-rtc",
                                    "loongson,ls-rtc"];
#[cfg(target_arch = "loongarch64")]
const SYS_TOYTRIM : usize = 0x20;
#[cfg(target_arch = "loongarch64")]
const SYS_TOY_READ0 : usize = 0x2C;
#[cfg(target_arch = "loongarch64")]
const SYS_TOY_READ1 : usize = 0x30;
#[cfg(target_arch = "loongarch64")]
const SYS_RTCCTRL : usize = 0x40;
#[cfg(target_arch = "loongarch64")]
const SYS_RTCTRIM : usize = 0x60;
#[cfg(target_arch = "loongarch64")]
const RTC_ENABLE : u32 = 1 << 13;
#[cfg(target_arch = "loongarch64")]
const TOY_ENABLE : u32 = 1 << 11;
#[cfg(target_arch = "loongarch64")]
const OSC_ENABLE : u32 = 1 << 8;
#[cfg(target_arch = "loongarch64")]
const ENABLE_MASK : u32 = RTC_ENABLE | TOY_ENABLE | OSC_ENABLE;

#[cfg(target_arch = "loongarch64")]
pub fn realtime_ns(dtb_pa : usize) -> DriverResult<u64> {
    let fdt = read_fdt(dtb_pa)?;
    let rtc = fdt.all_nodes()
                 .find(|node| {
                     let compatibles = compatible_list(node);
                     compatibles.iter()
                                .any(|compatible| {
                                    RTC_COMPATIBLES.iter()
                                                   .any(|item| *item == compatible.as_str())
                                })
                 })
                 .and_then(first_mmio_region)
                 .filter(|region| region.size >= SYS_RTCTRIM + core::mem::size_of::<u32>())
                 .ok_or(DriverError::NotFound)?;

    let unix_timestamp = read_unix_timestamp(rtc.base);
    if unix_timestamp == 0 {
        Err(DriverError::IoError)
    } else {
        Ok(unix_timestamp)
    }
}

#[cfg(not(target_arch = "loongarch64"))]
pub fn realtime_ns(_dtb_pa : usize) -> DriverResult<u64> { Err(DriverError::Unsupported) }

#[cfg(target_arch = "loongarch64")]
fn read_unix_timestamp(base : usize) -> u64 {
    let toy_trim = mmio_read32(base, SYS_TOYTRIM);
    let rtc_trim = mmio_read32(base, SYS_RTCTRIM);
    mmio_write32(base, SYS_TOYTRIM, 0);
    mmio_write32(base, SYS_RTCTRIM, 0);

    let ctrl = mmio_read32(base, SYS_RTCCTRL);
    mmio_write32(base, SYS_RTCCTRL, ctrl | ENABLE_MASK);
    let ctrl_after = mmio_read32(base, SYS_RTCCTRL);

    let mut last_toy_low = 0;
    let mut last_toy_high = 0;
    for _ in 0..3 {
        let toy_low = mmio_read32(base, SYS_TOY_READ0);
        let toy_high = mmio_read32(base, SYS_TOY_READ1);
        last_toy_low = toy_low;
        last_toy_high = toy_high;
        if let Some(unix_timestamp) = toy_to_unix_timestamp(toy_high, toy_low) {
            return unix_timestamp;
        }
        core::hint::spin_loop();
    }

    log::warn!("[driver][2k1000] RTC returned invalid TOY value: \
                ctrl={ctrl:#010x}->{ctrl_after:#010x}, toy_trim={toy_trim:#010x}, \
                rtc_trim={rtc_trim:#010x}, toy_high={last_toy_high:#010x}, \
                toy_low={last_toy_low:#010x}");
    0
}

#[cfg(target_arch = "loongarch64")]
fn mmio_read32(base : usize, byte_offset : usize) -> u32 {
    let ptr = base.wrapping_add(byte_offset) as *const u32;
    unsafe { core::ptr::read_volatile(ptr) }
}

#[cfg(target_arch = "loongarch64")]
fn mmio_write32(base : usize, byte_offset : usize, value : u32) {
    let ptr = base.wrapping_add(byte_offset) as *mut u32;
    unsafe { core::ptr::write_volatile(ptr, value) }
}

#[cfg(target_arch = "loongarch64")]
fn toy_to_unix_timestamp(toy_high : u32, toy_low : u32) -> Option<u64> {
    let year = 1900 + toy_high as i32;
    let month = extract_bits(toy_low, 26, 6);
    let day = extract_bits(toy_low, 21, 5);
    let hour = extract_bits(toy_low, 16, 5);
    let minute = extract_bits(toy_low, 10, 6);
    let second = extract_bits(toy_low, 4, 6);
    datetime_to_unix_timestamp(year, month, day, hour, minute, second)
}

#[cfg(target_arch = "loongarch64")]
fn extract_bits(value : u32, start : u32, width : u32) -> u32 {
    (value >> start) & ((1 << width) - 1)
}

#[allow(dead_code)]
fn datetime_to_unix_timestamp(year : i32,
                              month : u32,
                              day : u32,
                              hour : u32,
                              minute : u32,
                              second : u32)
                              -> Option<u64> {
    if !(1..=12).contains(&month) ||
       !(1..=31).contains(&day) ||
       hour > 23 ||
       minute > 59 ||
       second > 59 ||
       year < 1970
    {
        return None;
    }

    let mut days = 0u64;
    let mut y = 1970i32;
    while y < year {
        days += if is_leap_year(y) { 366 } else { 365 };
        y += 1;
    }
    let month_days = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    for m in 0..(month - 1) as usize {
        days += month_days[m] as u64;
    }
    if day as usize > month_days[(month - 1) as usize] {
        return None;
    }
    days += (day - 1) as u64;
    let secs = days * 86_400 + u64::from(hour) * 3_600 + u64::from(minute) * 60 + u64::from(second);
    Some(secs * 1_000_000_000)
}

#[allow(dead_code)]
fn is_leap_year(year : i32) -> bool { (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 }

/// 纯函数自检：验证 LS2K1000 TOY 位域解码。
pub fn test() {
    #[cfg(target_arch = "loongarch64")]
    {
        let toy_high = 125;
        let toy_low = (6 << 26) | (12 << 21) | (8 << 16) | (34 << 10) | (56 << 4);
        assert_eq!(toy_to_unix_timestamp(toy_high, toy_low),
                   Some(1_749_717_296_000_000_000));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toy_decodes_wall_time_to_unix_timestamp() {
        #[cfg(target_arch = "loongarch64")]
        test();
    }

    #[test]
    fn toy_rejects_invalid_zero_timestamp() {
        assert!(datetime_to_unix_timestamp(1969, 1, 1, 0, 0, 0).is_none());
    }
}
