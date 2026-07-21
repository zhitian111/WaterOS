//! 时钟和计时器相关的 syscall 实现。

pub(crate) mod clock;
pub(crate) mod rtc;

pub(crate) use clock::{
    sys_adjtimex, sys_clock_adjtime, sys_clock_getres, sys_clock_gettime, sys_clock_nanosleep,
    sys_clock_settime, sys_gettimeofday, sys_nanosleep,
};
pub(crate) use rtc::sys_rtc_ioctl;
