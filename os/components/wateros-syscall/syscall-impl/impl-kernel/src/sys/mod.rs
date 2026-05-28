//! 各 Linux 风格系统调用的 `sys_*` 实现；由 [`crate::KernelSyscallDispatcher`]
//! 按号表路由。

mod brk;
mod chdir;
mod clone;
mod close;
mod dup;
mod execve;
mod fcntl;
mod fstat;
mod futex;
mod getcwd;
mod getdents64;
mod kill;
mod lseek;
mod mkdirat;
mod mmap;
mod mount;
mod openat;
mod path_at;
mod pipe2;
mod unlinkat;
mod umount2;
mod read;
mod task;
mod write;

pub(crate) use brk::sys_brk;
pub(crate) use chdir::sys_chdir;
pub(crate) use clone::sys_clone;
pub(crate) use close::sys_close;
pub(crate) use dup::{sys_dup, sys_dup3};
pub(crate) use execve::sys_execve;
pub(crate) use fcntl::sys_fcntl;
pub(crate) use fstat::{sys_fstat, sys_statx};
pub(crate) use futex::sys_futex;
pub(crate) use getcwd::sys_getcwd;
pub(crate) use getdents64::sys_getdents64;
pub(crate) use kill::sys_kill;
pub(crate) use lseek::sys_lseek;
pub(crate) use mkdirat::sys_mkdirat;
pub(crate) use mmap::{sys_mmap, sys_mprotect, sys_munmap};
pub(crate) use mount::sys_mount;
pub(crate) use openat::sys_openat;
pub(crate) use pipe2::sys_pipe2;
pub(crate) use unlinkat::sys_unlinkat;
pub(crate) use umount2::sys_umount2;
pub(crate) use read::sys_read;
pub(crate) use task::{
    sys_clock_gettime, sys_exit, sys_getpid, sys_getppid, sys_getrlimit, sys_gettid,
    sys_gettimeofday, sys_nanosleep, sys_prctl, sys_set_tid_address, sys_setrlimit, sys_times,
    sys_uname, sys_waitpid, sys_yield,
};
pub(crate) use write::sys_write;
