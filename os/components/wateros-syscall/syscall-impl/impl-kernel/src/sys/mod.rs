//! 各 Linux 风格系统调用的 `sys_*` 实现；由 [`crate::KernelSyscallDispatcher`]
//! 按号表路由。

mod brk;
mod close;
mod fstat;
mod lseek;
mod mmap;
mod openat;
mod pipe2;
mod read;
mod task;
mod write;

pub(crate) use brk::sys_brk;
pub(crate) use close::sys_close;
pub(crate) use fstat::sys_fstat;
pub(crate) use lseek::sys_lseek;
pub(crate) use mmap::{sys_mmap, sys_mprotect, sys_munmap};
pub(crate) use openat::sys_openat;
pub(crate) use pipe2::sys_pipe2;
pub(crate) use read::sys_read;
pub(crate) use task::{
    sys_clock_gettime, sys_exit, sys_getpid, sys_getppid, sys_gettimeofday, sys_nanosleep,
    sys_set_tid_address, sys_times, sys_waitpid, sys_yield,
};
pub(crate) use write::sys_write;
