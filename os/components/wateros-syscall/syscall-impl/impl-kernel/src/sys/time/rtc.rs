//! `/dev/misc/rtc` 等软件 RTC 的 `ioctl(2)` 处理。

//! 本模块代码由AI完成
use api_v0::ErrNo;
use api_v0::UserRet;
use platform::wall_clock::{ns_to_rtc_time, realtime_ns, rtc_time_to_ns, set_realtime_ns};

use crate::user_copy::{copy_from_user_struct, copy_to_user_struct};

const RTC_RD_TIME: u32 = 0x8024_7009;
const RTC_SET_TIME: u32 = 0x4024_700a;

#[repr(C)]
#[derive(Clone, Copy)]
struct UserRtcTime {
    tm_sec: i32,
    tm_min: i32,
    tm_hour: i32,
    tm_mday: i32,
    tm_mon: i32,
    tm_year: i32,
    tm_wday: i32,
    tm_yday: i32,
    tm_isdst: i32,
}

impl From<platform::wall_clock::RtcTimeFields> for UserRtcTime {
    fn from(f: platform::wall_clock::RtcTimeFields) -> Self {
        Self {
            tm_sec: f.tm_sec,
            tm_min: f.tm_min,
            tm_hour: f.tm_hour,
            tm_mday: f.tm_mday,
            tm_mon: f.tm_mon,
            tm_year: f.tm_year,
            tm_wday: f.tm_wday,
            tm_yday: f.tm_yday,
            tm_isdst: f.tm_isdst,
        }
    }
}

impl From<UserRtcTime> for platform::wall_clock::RtcTimeFields {
    fn from(u: UserRtcTime) -> Self {
        Self {
            tm_sec: u.tm_sec,
            tm_min: u.tm_min,
            tm_hour: u.tm_hour,
            tm_mday: u.tm_mday,
            tm_mon: u.tm_mon,
            tm_year: u.tm_year,
            tm_wday: u.tm_wday,
            tm_yday: u.tm_yday,
            tm_isdst: u.tm_isdst,
        }
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_rtc_ioctl(request: u32, argp: usize) -> UserRet {
    match request {
        RTC_RD_TIME => {
            if argp == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            let ns = match realtime_ns() {
                Ok(ns) => ns,
                Err(()) => return UserRet::from_error(ErrNo::EIO),
            };
            let rtc = UserRtcTime::from(ns_to_rtc_time(ns));
            match copy_to_user_struct(argp, &rtc) {
                Ok(()) => UserRet::from_success(0),
                Err(e) => UserRet::from_error(e),
            }
        }
        RTC_SET_TIME => {
            if argp == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            let user = match copy_from_user_struct::<UserRtcTime>(argp) {
                Ok(v) => v,
                Err(e) => return UserRet::from_error(e),
            };
            let fields: platform::wall_clock::RtcTimeFields = user.into();
            let target_ns = match rtc_time_to_ns(&fields) {
                Ok(ns) => ns,
                Err(()) => return UserRet::from_error(ErrNo::EINVAL),
            };
            if set_realtime_ns(target_ns).is_err() {
                return UserRet::from_error(ErrNo::EIO);
            }
            UserRet::from_success(0)
        }
        _ => UserRet::from_error(ErrNo::ENOTTY),
    }
}
