//! 各 Linux 风格系统调用的 `sys_*` 实现；由 [`crate::KernelSyscallDispatcher`]
//! 按号表路由。

mod brk;
mod chdir;
mod clock;
mod clone;
mod close;
mod cred;
mod dup;
mod execve;
mod faccessat;
mod fcntl;
mod fstat;
mod futex;
mod getcwd;
mod getdents64;
mod ioctl;
mod kill;
mod lseek;
mod mkdirat;
mod mmap;
mod mount;
mod openat;
mod path_at;
mod pipe2;
mod poll;
mod poll_multiplex;
mod posix_at_io;
mod read;
mod readlinkat;
mod renameat2;
mod sendfile;
mod robust;
mod syslog;
mod task;
mod umount2;
mod unlinkat;
mod utimensat;
mod write;

// socket / 网络
mod accept;
mod bind;
mod connect;
mod listen;
mod recvfrom;
mod sendmsg;
mod sendto;
mod shutdown;
mod socket;
mod sockname;
mod sockopt;

pub(crate) use brk::sys_brk;
pub(crate) use chdir::sys_chdir;
pub(crate) use clone::sys_clone;
pub(crate) use close::sys_close;
pub(crate) use cred::{
    sys_getegid, sys_geteuid, sys_getgid, sys_getgroups, sys_getuid, sys_setgid, sys_setregid,
    sys_setresgid, sys_setresuid, sys_setreuid, sys_setuid,
};
pub(crate) use dup::{sys_dup, sys_dup3};
pub(crate) use execve::sys_execve;
pub(crate) use faccessat::{sys_faccessat, sys_faccessat2};
pub(crate) use fcntl::sys_fcntl;
pub(crate) use fstat::{sys_fstat, sys_fstatat, sys_statx};
pub(crate) use futex::{sys_futex, wake_user_addr};
pub(crate) use robust::{
    robust_exit_cleanup, robust_exit_cleanup_siblings_for_exec, sys_get_robust_list,
    sys_set_robust_list,
};
pub(crate) use getcwd::sys_getcwd;
pub(crate) use getdents64::sys_getdents64;
pub(crate) use ioctl::sys_ioctl;
pub(crate) use kill::sys_kill;
pub(crate) use lseek::sys_lseek;
pub(crate) use mkdirat::sys_mkdirat;
pub(crate) use mmap::{sys_mmap, sys_mprotect, sys_munmap};
pub(crate) use mount::sys_mount;
pub(crate) use openat::sys_openat;
pub(crate) use pipe2::sys_pipe2;
pub(crate) use poll::sys_poll;
pub(crate) use poll_multiplex::{sys_ppoll, sys_pselect6, sys_select};
pub(crate) use posix_at_io::{sys_pread64, sys_preadv, sys_pwrite64, sys_pwritev};
pub(crate) use read::sys_read;
pub(crate) use readlinkat::sys_readlinkat;
pub(crate) use renameat2::sys_renameat2;
pub(crate) use sendfile::sys_sendfile;
pub(crate) use clock::{
    sys_clock_getres, sys_clock_gettime, sys_clock_nanosleep, sys_clock_settime, sys_gettimeofday,
    sys_nanosleep,
};
pub(crate) use syslog::sys_syslog;
pub(crate) use task::{
    sys_exit, sys_exit_group, sys_getpid, sys_getppid, sys_getrandom,
    sys_getrlimit, sys_getrusage, sys_gettid, sys_prctl, sys_prlimit64, sys_sysinfo,
    sys_rt_sigaction, sys_rt_sigprocmask, sys_rt_sigtimedwait, sys_set_tid_address,
    sys_setitimer, sys_setrlimit, sys_times, sys_uname, sys_waitpid, sys_yield,
};
pub(crate) use umount2::sys_umount2;
pub(crate) use unlinkat::sys_unlinkat;
pub(crate) use utimensat::sys_utimensat;
pub(crate) use write::{sys_write, sys_writev};

// socket / 网络
pub(crate) use accept::sys_accept4;
pub(crate) use bind::sys_bind;
pub(crate) use connect::sys_connect;
pub(crate) use listen::sys_listen;
pub(crate) use recvfrom::sys_recvfrom;
pub(crate) use sendmsg::{sys_recvmsg, sys_sendmsg};
pub(crate) use sendto::sys_sendto;
pub(crate) use shutdown::sys_shutdown;
pub(crate) use socket::sys_socket;
pub(crate) use sockname::{sys_getpeername, sys_getsockname};
pub(crate) use sockopt::{sys_getsockopt, sys_setsockopt};
