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
    pub tm_sec: i32,
    pub tm_min: i32,
    pub tm_hour: i32,
    pub tm_mday: i32,
    pub tm_mon: i32,
    pub tm_year: i32,
    pub tm_wday: i32,
    pub tm_yday: i32,
    pub tm_isdst: i32,
}

/// 标记型 RTC 字符设备；实际 `RTC_*` ioctl 在 syscall 层对 rtc fd 分发。
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
