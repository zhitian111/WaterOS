//! 各 Linux 风格系统调用的 `sys_*` 实现；由 [`crate::dispatch`] 按号表路由。

mod brk;
mod close;
mod mmap;
mod pipe2;
mod read;
mod task;
mod write;

pub(crate) use brk::sys_brk;
pub(crate) use close::sys_close;
pub(crate) use mmap::{sys_mmap, sys_mprotect, sys_munmap};
pub(crate) use pipe2::sys_pipe2;
pub(crate) use read::sys_read;
pub(crate) use task::{sys_exit, sys_get_time, sys_getpid, sys_nanosleep, sys_waitpid, sys_yield};
pub(crate) use write::sys_write;
