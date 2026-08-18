//! 本模块代码由AI完成
//! 软件 RTC 字符设备 stub：`hwclock` 等通过 `/dev/misc/rtc` + `ioctl` 访问；时间语义由 syscall 层处理。

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::sync::Arc;
use character_api_v0::{
    register_character_device, CharacterDevice, CharacterDeviceKind, SharedCharacterDevice,
};
use driver_api::{DriverError, DriverResult};
use spin::Mutex;

/// Linux `struct rtc_time`（与 glibc/musl hwclock 布局一致）。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RtcTime {
    /// 秒，范围通常为 0..=59。
    pub tm_sec: i32,
    /// 分钟，范围 0..=59。
    pub tm_min: i32,
    /// 小时，范围 0..=23。
    pub tm_hour: i32,
    /// 月内日期，从 1 开始。
    pub tm_mday: i32,
    /// 月份，从 0 表示一月。
    pub tm_mon: i32,
    /// 自 1900 年起的年份偏移。
    pub tm_year: i32,
    /// 星期日为 0 的星期编号。
    pub tm_wday: i32,
    /// 年内日，从 0 开始。
    pub tm_yday: i32,
    /// 夏令时标志，-1 表示未知。
    pub tm_isdst: i32,
}

/// 标记型 RTC 字符设备；实际 `RTC_*` ioctl 在 syscall 层对 rtc fd 分发。
// 本结构代码由AI完成
#[derive(Debug, Clone, Copy, Default)]
pub struct RtcCharacterDevice;

impl CharacterDevice for RtcCharacterDevice {
    fn read(&mut self, _buf: &mut [u8]) -> DriverResult<usize> {
        Ok(0)
    }

    fn write(&mut self, buf: &[u8]) -> DriverResult<usize> {
        let _ = buf;
        Err(DriverError::Unsupported)
    }

    fn device_kind(&self) -> CharacterDeviceKind {
        CharacterDeviceKind::Rtc
    }
}

/// 注册全局 RTC stub 并返回设备索引。
pub fn register_rtc_stub() -> usize {
    let shared: SharedCharacterDevice =
        Arc::new(Mutex::new(Box::new(RtcCharacterDevice)));
    register_character_device(shared)
}
