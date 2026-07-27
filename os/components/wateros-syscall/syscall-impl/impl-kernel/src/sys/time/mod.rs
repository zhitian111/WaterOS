//! 时钟和计时器相关的 syscall 实现。

pub(crate) mod clock;
pub(crate) mod posix_timer;
pub(crate) mod rtc;
pub(crate) mod timer;

pub(crate) use clock::{
    sys_adjtimex, sys_clock_adjtime, sys_clock_getres, sys_clock_gettime, sys_clock_nanosleep,
    sys_clock_settime, sys_gettimeofday, sys_nanosleep,
};
pub(crate) use posix_timer::{
    sys_timer_create, sys_timer_delete, sys_timer_getoverrun, sys_timer_gettime, sys_timer_settime,
};
pub(crate) use rtc::sys_rtc_ioctl;
pub(crate) use timer::{sys_getitimer, sys_getrusage, sys_setitimer, sys_times};
