//! VisionFive 2 / JH7110 RTC 读取与墙上时钟初始化。

use api_v0::{DriverError, DriverResult};

#[cfg(target_arch = "riscv64")]
use common::dtb::{compatible_list, first_mmio_region, read_fdt};

#[cfg(target_arch = "riscv64")]
const RTC_TIME_OFFSET : usize = 0x3C;
#[cfg(target_arch = "riscv64")]
const RTC_DATE_OFFSET : usize = 0x40;
#[cfg(target_arch = "riscv64")]
const COMPATIBLE : &str = "starfive,jh7110-rtc";

#[cfg(target_arch = "riscv64")]
pub fn realtime_ns(dtb_pa : usize) -> DriverResult<u64> {
    let fdt = read_fdt(dtb_pa)?;
    let rtc = fdt.all_nodes()
                 .find(|node| {
                     compatible_list(node).iter()
                                          .any(|item| item == COMPATIBLE)
                 })
                 .and_then(first_mmio_region)
                 .filter(|region| region.size >= RTC_DATE_OFFSET + core::mem::size_of::<u32>())
                 .ok_or(DriverError::NotFound)?;

    for _ in 0..3 {
        let time_reg = mmio_read32(rtc.base, RTC_TIME_OFFSET);
        let date_reg = mmio_read32(rtc.base, RTC_DATE_OFFSET);
        if let Some(ns) = decode_rtc_datetime(time_reg, date_reg) {
            return Ok(ns);
        }
        core::hint::spin_loop();
    }
    Err(DriverError::IoError)
}

#[cfg(not(target_arch = "riscv64"))]
pub fn realtime_ns(_dtb_pa : usize) -> DriverResult<u64> { Err(DriverError::Unsupported) }

#[cfg(target_arch = "riscv64")]
fn mmio_read32(base : usize, byte_offset : usize) -> u32 {
    let ptr = base.wrapping_add(byte_offset) as *const u32;
    unsafe { core::ptr::read_volatile(ptr) }
}

fn decode_rtc_datetime(time_reg : u32, date_reg : u32) -> Option<u64> {
    let second = time_reg & 0x7F;
    let minute = (time_reg >> 7) & 0x7F;
    let hour = (time_reg >> 14) & 0x7F;
    let day = date_reg & 0x3F;
    let month = (date_reg >> 6) & 0x1F;
    let year_since_2000 = (date_reg >> 11) & 0xFF;

    if !(1..=99).contains(&year_since_2000) {
        return None;
    }

    datetime_to_unix_timestamp(2000 + year_since_2000 as i32,
                               month,
                               day,
                               hour,
                               minute,
                               second)
}

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

fn is_leap_year(year : i32) -> bool { (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 }

/// 纯函数自检：验证 JH7110 RTC 的位域解码。
pub fn test() {
    let time_reg = 56 | (34 << 7) | (8 << 14);
    let date_reg = 12 | (6 << 6) | (25 << 11);
    assert_eq!(decode_rtc_datetime(time_reg, date_reg),
               Some(1_749_717_296_000_000_000));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_valid_wall_clock() { test(); }

    #[test]
    fn rejects_invalid_year() {
        assert_eq!(decode_rtc_datetime(0, 0), None);
    }
}
