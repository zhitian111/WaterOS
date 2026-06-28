//! 各 Linux 风格系统调用的 `sys_*` 实现；由 [`crate::KernelSyscallDispatcher`]
//! 按号表路由。

mod brk;
mod bringup_stats;
mod cap;
mod chdir;
mod clock;
mod clone;
mod close;
mod close_range;
mod cred;
mod dup;
mod epoll;
mod execve;
mod faccessat;
mod fchmodat;
mod fchownat;
mod fcntl;
mod fallocate;
mod flock;
mod fstat;
mod ftruncate;
mod futex;
mod getcwd;
mod getdents64;
mod ioctl;
mod kill;
mod lseek;
mod ltp_cgroup_helper;
mod mkdirat;
mod mempolicy;
mod mmap;
mod mount;
mod openat;
mod path_at;
mod pipe2;
mod poll;
mod poll_multiplex;
mod posix_at_io;
mod priority;
mod read;
mod readlinkat;
mod renameat2;
mod rtc;
mod sendfile;
mod robust;
mod sched;
mod signal;
mod syslog;
#[path = "../stat_times.rs"]
mod stat_times;
mod statfs;
mod symlinkat;
mod sync;
mod task;
mod truncate;
mod umount2;
mod unshare;
mod unlinkat;
mod utimensat;
mod write;
mod xattr;

// socket / 网络
mod accept;
mod acct;
mod bind;
mod connect;
mod listen;
mod recvfrom;
mod sendmsg;
mod sendto;
mod shm;
mod shutdown;
mod socket;
mod socketpair;
mod sockname;
mod sockopt;

pub(crate) use brk::sys_brk;
pub(crate) use cap::{sys_capget, sys_capset};
pub(crate) use chdir::sys_chdir;
pub(crate) use clone::{sys_clone, sys_clone3};
pub(crate) use close::sys_close;
pub(crate) use close_range::sys_close_range;
pub(crate) use cred::{
    sys_getegid, sys_geteuid, sys_getgid, sys_getgroups, sys_getresgid, sys_getresuid, sys_getuid,
    sys_setgid, sys_setgroups, sys_setregid, sys_setresgid, sys_setresuid, sys_setreuid, sys_setuid,
};
pub(crate) use dup::{sys_dup, sys_dup3};
pub(crate) use epoll::{sys_epoll_create1, sys_epoll_ctl, sys_epoll_pwait, sys_epoll_wait};
pub(crate) use execve::sys_execve;
pub(crate) use faccessat::{sys_faccessat, sys_faccessat2};
pub(crate) use fchmodat::sys_fchmodat;
pub(crate) use fchownat::sys_fchownat;
pub(crate) use fcntl::sys_fcntl;
pub(crate) use flock::sys_flock;
pub(crate) use fallocate::sys_fallocate;
pub(crate) use fstat::{sys_fstat, sys_fstatat, sys_statx};
pub(crate) use ftruncate::sys_ftruncate;
pub(crate) use futex::{sys_futex, wake_user_addr};
pub(crate) use robust::{
    robust_exit_cleanup, robust_exit_cleanup_siblings_for_exec, sys_get_robust_list,
    sys_set_robust_list,
};
pub(crate) use sched::{
    sys_sched_get_priority_max, sys_sched_get_priority_min, sys_sched_getaffinity,
    sys_sched_getattr, sys_sched_getparam, sys_sched_getscheduler, sys_sched_setaffinity,
    sys_sched_setattr, sys_sched_setparam, sys_sched_setscheduler,
};
pub(crate) use signal::{
    deliver_pending_signal, restore_signal_frame, sys_rt_sigpending, sys_rt_sigsuspend,
    sys_tgkill, sys_tkill, timer_tick, raise_current_thread,
};
pub(crate) use getcwd::sys_getcwd;
pub(crate) use getdents64::sys_getdents64;
pub(crate) use ioctl::sys_ioctl;
pub(crate) use kill::sys_kill;
pub(crate) use lseek::sys_lseek;
pub(crate) use mkdirat::sys_mkdirat;
pub(crate) use mempolicy::sys_get_mempolicy;
pub(crate) use mmap::{
    sys_madvise, sys_mmap, sys_mlock, sys_mlockall, sys_mprotect, sys_mremap, sys_msync, sys_munlock,
    sys_munlockall, sys_munmap,
};
pub(crate) use mount::sys_mount;
pub(crate) use openat::sys_openat;
pub(crate) use pipe2::sys_pipe2;
pub(crate) use poll::sys_poll;
pub(crate) use poll_multiplex::{sys_ppoll, sys_pselect6, sys_select};
pub(crate) use posix_at_io::{sys_pread64, sys_preadv, sys_pwrite64, sys_pwritev};
pub(crate) use read::{sys_read, sys_readv};
pub(crate) use readlinkat::sys_readlinkat;
pub(crate) use renameat2::sys_renameat2;
pub(crate) use sendfile::sys_sendfile;
pub(crate) use shm::{sys_shmat, sys_shmctl, sys_shmdt, sys_shmget};
pub(crate) use clock::{
    sys_adjtimex, sys_clock_adjtime, sys_clock_getres, sys_clock_gettime, sys_clock_nanosleep,
    sys_clock_settime, sys_gettimeofday, sys_nanosleep,
};
pub(crate) use statfs::sys_statfs;
pub(crate) use symlinkat::sys_symlinkat;
pub(crate) use sync::{sys_fdatasync, sys_fsync, sys_sync};
pub(crate) use truncate::sys_truncate;
pub(crate) use syslog::sys_syslog;
pub(crate) use task::{
    drop_reaped_task_runtime_resources, sys_exit, sys_exit_group, sys_getpid, sys_getppid, sys_getrandom,
    sys_getrlimit, sys_getrusage, sys_gettid, sys_prctl, sys_prlimit64, sys_sysinfo,
    sys_rt_sigaction, sys_rt_sigprocmask, sys_rt_sigtimedwait, sys_set_tid_address,
    sys_getitimer, sys_setitimer, sys_setrlimit, sys_setpgid, sys_getpgid, sys_times, sys_umask,
    sys_uname, sys_waitpid, sys_setsid, sys_yield,
};
pub(crate) use priority::{sys_getpriority, sys_setpriority};
pub(crate) use umount2::sys_umount2;
pub(crate) use unshare::sys_unshare;
pub(crate) use unlinkat::sys_unlinkat;
pub(crate) use utimensat::sys_utimensat;
pub(crate) use xattr::{
    sys_fgetxattr, sys_flistxattr, sys_fremovexattr, sys_fsetxattr, sys_getxattr, sys_listxattr,
    sys_lgetxattr, sys_llistxattr, sys_lremovexattr, sys_lsetxattr, sys_removexattr, sys_setxattr,
};
pub(crate) use write::{sys_write, sys_writev};

// socket / 网络
pub(crate) use accept::{sys_accept, sys_accept4};
pub(crate) use acct::sys_acct;
pub(crate) use bind::sys_bind;
pub(crate) use connect::sys_connect;
pub(crate) use listen::sys_listen;
pub(crate) use recvfrom::sys_recvfrom;
pub(crate) use sendmsg::{sys_recvmsg, sys_sendmsg};
pub(crate) use sendto::sys_sendto;
pub(crate) use shutdown::sys_shutdown;
pub(crate) use socket::sys_socket;
pub(crate) use socketpair::sys_socketpair;
pub(crate) use sockname::{sys_getpeername, sys_getsockname};
pub(crate) use sockopt::{sys_getsockopt, sys_setsockopt};

pub use bringup_stats::{log_thread_bringup_stats_summary, record_user_page_fault_handled};
