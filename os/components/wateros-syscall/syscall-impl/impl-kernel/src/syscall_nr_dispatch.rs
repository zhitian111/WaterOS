//! 按裸 syscall 号分发的稠密函数指针表。
//!
//! 当前分发等价于 Linux 各架构的 `sys_call_table`：按调用号直接索引函数指针，
//! 未实现的槽位统一由 [`sys_enosys`] 返回 `ENOSYS`。绝大多数 handler 都保持
//! 原有的 `fn(SyscallArgs) -> UserRet` 签名直接放入主表，只有少数签名不一致的
//! 调用经过第二张特殊表适配，避免在热路径上增加额外的 wrapper 调用。

use crate::sys;
use api_v0::SyscallArgs;
use api_v0::UserRet;

type ArgHandler = fn(SyscallArgs) -> UserRet;
type SpecialHandler = fn(SyscallArgs) -> isize;

/// 当前已登记的最大 asm-generic64 syscall 号加一。
const SYSCALL_TABLE_SIZE : usize = api_v0::EPOLL_PWAIT2 + 1;

#[inline]
fn sys_enosys(_args : SyscallArgs) -> isize { api_v0::UserRet::from_error(api_v0::ErrNo::ENOSYS).0 }

macro_rules! arg_syscalls {
    ($($nr:expr => $handler:path),* $(,)?) => {
        const ARG_SYSCALL_TABLE : [Option<ArgHandler>; SYSCALL_TABLE_SIZE] = {
            let mut table = [None; SYSCALL_TABLE_SIZE];
            $(
                table[$nr] = Some($handler as ArgHandler);
            )*
            table
        };
    };
}

macro_rules! noarg_syscalls {
    ($($name:ident => $handler:path),* $(,)?) => {
        $(
            #[inline]
            fn $name(_args : SyscallArgs) -> isize { $handler().0 }
        )*
    };
}

arg_syscalls! {
    api_v0::SCHED_SETPARAM => sys::sys_sched_setparam,
    api_v0::SCHED_SETSCHEDULER => sys::sys_sched_setscheduler,
    api_v0::SCHED_GETSCHEDULER => sys::sys_sched_getscheduler,
    api_v0::SCHED_GETPARAM => sys::sys_sched_getparam,
    api_v0::SCHED_SETAFFINITY => sys::sys_sched_setaffinity,
    api_v0::SCHED_GETAFFINITY => sys::sys_sched_getaffinity,
    api_v0::SCHED_GET_PRIORITY_MAX => sys::sys_sched_get_priority_max,
    api_v0::SCHED_GET_PRIORITY_MIN => sys::sys_sched_get_priority_min,
    api_v0::GETCPU => sys::sys_getcpu,
    api_v0::READ => sys::sys_read,
    api_v0::READV => sys::sys_readv,
    api_v0::WRITE => sys::sys_write,
    api_v0::WRITEV => sys::sys_writev,
    api_v0::PREAD64 => sys::sys_pread64,
    api_v0::PWRITE64 => sys::sys_pwrite64,
    api_v0::PREADV => sys::sys_preadv,
    api_v0::PWRITEV => sys::sys_pwritev,
    api_v0::PREADV2 => sys::sys_preadv2,
    api_v0::PWRITEV2 => sys::sys_pwritev2,
    api_v0::SENDFILE => sys::sys_sendfile,
    api_v0::SPLICE => sys::sys_splice,
    api_v0::TEE => sys::sys_tee,
    api_v0::VMSPLICE => sys::sys_vmsplice,
    api_v0::COPY_FILE_RANGE => sys::sys_copy_file_range,
    api_v0::FADVISE64 => sys::sys_fadvise64,
    api_v0::READAHEAD => sys::sys_readahead,
    api_v0::READLINKAT => sys::sys_readlinkat,
    api_v0::FACCESSAT => sys::sys_faccessat,
    api_v0::FCHDIR => sys::sys_fchdir,
    api_v0::FCHMOD => sys::sys_fchmod,
    api_v0::FCHMODAT => sys::sys_fchmodat,
    api_v0::FCHOWN => sys::sys_fchown,
    api_v0::FCHOWNAT => sys::sys_fchownat,
    api_v0::STATFS => sys::sys_statfs,
    api_v0::SYNC => sys::sys_sync,
    api_v0::SYNCFS => sys::sys_syncfs,
    api_v0::FSYNC => sys::sys_fsync,
    api_v0::FDATASYNC => sys::sys_fdatasync,
    api_v0::TRUNCATE => sys::sys_truncate,
    api_v0::FTRUNCATE => sys::sys_ftruncate,
    api_v0::FALLOCATE => sys::sys_fallocate,
    api_v0::OPENAT => sys::sys_openat,
    api_v0::OPENAT2 => sys::sys_openat2,
    api_v0::CLOSE => sys::sys_close,
    api_v0::FSTAT => sys::sys_fstat,
    api_v0::LSEEK => sys::sys_lseek,
    api_v0::DUP => sys::sys_dup,
    api_v0::DUP3 => sys::sys_dup3,
    api_v0::PIPE2 => sys::sys_pipe2,
    api_v0::EVENTFD2 => sys::sys_eventfd2,
    api_v0::TIMERFD_CREATE => sys::sys_timerfd_create,
    api_v0::TIMERFD_SETTIME => sys::sys_timerfd_settime,
    api_v0::TIMERFD_GETTIME => sys::sys_timerfd_gettime,
    api_v0::IOCTL => sys::sys_ioctl,
    api_v0::FCNTL => sys::sys_fcntl,
    api_v0::FLOCK => sys::sys_flock,
    api_v0::GETDENTS64 => sys::sys_getdents64,
    api_v0::MKDIRAT => sys::sys_mkdirat,
    api_v0::SYMLINKAT => sys::sys_symlinkat,
    api_v0::UNLINKAT => sys::sys_unlinkat,
    api_v0::RENAMEAT => sys::sys_renameat,
    api_v0::RENAMEAT2 => sys::sys_renameat2,
    api_v0::UTIMENSAT => sys::sys_utimensat,
    api_v0::MOUNT => sys::sys_mount,
    api_v0::UMOUNT2 => sys::sys_umount2,
    api_v0::FORK => sys::sys_clone,
    api_v0::CLONE3 => sys::sys_clone3,
    api_v0::PIDFD_OPEN => sys::sys_pidfd_open,
    api_v0::PIDFD_SEND_SIGNAL => sys::sys_pidfd_send_signal,
    api_v0::PIDFD_GETFD => sys::sys_pidfd_getfd,
    api_v0::UNSHARE => sys::sys_unshare,
    api_v0::EXEC => sys::sys_execve,
    api_v0::MMAP => sys::sys_mmap,
    api_v0::MUNMAP => sys::sys_munmap,
    api_v0::MSYNC => sys::sys_msync,
    api_v0::MPROTECT => sys::sys_mprotect,
    api_v0::MREMAP => sys::sys_mremap,
    api_v0::MADVISE => sys::sys_madvise,
    api_v0::MLOCK => sys::sys_mlock,
    api_v0::MUNLOCK => sys::sys_munlock,
    api_v0::MLOCKALL => sys::sys_mlockall,
    api_v0::MUNLOCKALL => sys::sys_munlockall,
    api_v0::MINCORE => sys::sys_mincore,
    api_v0::GET_MEMPOLICY => sys::sys_get_mempolicy,
    api_v0::SHMGET => sys::sys_shmget,
    api_v0::SHMCTL => sys::sys_shmctl,
    api_v0::SHMAT => sys::sys_shmat,
    api_v0::SHMDT => sys::sys_shmdt,
    api_v0::MSGGET => sys::sys_msgget,
    api_v0::MSGCTL => sys::sys_msgctl,
    api_v0::MSGRCV => sys::sys_msgrcv,
    api_v0::MSGSND => sys::sys_msgsnd,
    api_v0::SEMGET => sys::sys_semget,
    api_v0::SEMCTL => sys::sys_semctl,
    api_v0::SEMTIMEDOP => sys::sys_semtimedop,
    api_v0::SEMOP => sys::sys_semop,
    api_v0::GET_TIME => sys::sys_gettimeofday,
    api_v0::CLOCK_SETTIME => sys::sys_clock_settime,
    api_v0::CLOCK_GETTIME => sys::sys_clock_gettime,
    api_v0::CLOCK_GETRES => sys::sys_clock_getres,
    api_v0::CLOCK_NANOSLEEP => sys::sys_clock_nanosleep,
    api_v0::TIMER_CREATE => sys::sys_timer_create,
    api_v0::TIMER_GETTIME => sys::sys_timer_gettime,
    api_v0::TIMER_GETOVERRUN => sys::sys_timer_getoverrun,
    api_v0::TIMER_SETTIME => sys::sys_timer_settime,
    api_v0::TIMER_DELETE => sys::sys_timer_delete,
    api_v0::GETSID => sys::sys_getsid,
    api_v0::GETGROUPS => sys::sys_getgroups,
    api_v0::SYSINFO => sys::sys_sysinfo,
    api_v0::SETUID => sys::sys_setuid,
    api_v0::SETGID => sys::sys_setgid,
    api_v0::SETREUID => sys::sys_setreuid,
    api_v0::SETREGID => sys::sys_setregid,
    api_v0::SETRESUID => sys::sys_setresuid,
    api_v0::SETRESGID => sys::sys_setresgid,
    api_v0::GETRESUID => sys::sys_getresuid,
    api_v0::GETRESGID => sys::sys_getresgid,
    api_v0::TIMES => sys::sys_times,
    api_v0::SETPGID => sys::sys_setpgid,
    api_v0::GETPGID => sys::sys_getpgid,
    api_v0::SETPRIORITY => sys::sys_setpriority,
    api_v0::GETPRIORITY => sys::sys_getpriority,
    api_v0::IOPRIO_SET => sys::sys_ioprio_set,
    api_v0::IOPRIO_GET => sys::sys_ioprio_get,
    api_v0::WAITPID => sys::sys_waitpid,
    api_v0::WAITID => sys::sys_waitid,
    api_v0::KILL => sys::sys_kill,
    api_v0::NANOSLEEP => sys::sys_nanosleep,
    api_v0::UNAME => sys::sys_uname,
    api_v0::PERSONALITY => sys::sys_personality,
    api_v0::SETHOSTNAME => sys::sys_sethostname,
    api_v0::SETDOMAINNAME => sys::sys_setdomainname,
    api_v0::REBOOT => sys::sys_reboot,
    api_v0::SYSLOG => sys::sys_syslog,
    api_v0::PRCTL => sys::sys_prctl,
    api_v0::CAPGET => sys::sys_capget,
    api_v0::CAPSET => sys::sys_capset,
    api_v0::GETCWD => sys::sys_getcwd,
    api_v0::CHDIR => sys::sys_chdir,
    api_v0::CHROOT => sys::sys_chroot,
    api_v0::FUTEX => sys::sys_futex,
    api_v0::SIGALTSTACK => sys::sys_sigaltstack,
    api_v0::RT_SIGACTION => sys::sys_rt_sigaction,
    api_v0::RT_SIGPROCMASK => sys::sys_rt_sigprocmask,
    api_v0::RT_SIGPENDING => sys::sys_rt_sigpending,
    api_v0::RT_SIGSUSPEND => sys::sys_rt_sigsuspend,
    api_v0::RT_SIGTIMEDWAIT => sys::sys_rt_sigtimedwait,
    api_v0::TKILL => sys::sys_tkill,
    api_v0::TGKILL => sys::sys_tgkill,
    api_v0::SET_TID_ADDRESS => sys::sys_set_tid_address,
    api_v0::SET_ROBUST_LIST => sys::sys_set_robust_list,
    api_v0::GET_ROBUST_LIST => sys::sys_get_robust_list,
    api_v0::RSEQ => sys::sys_rseq,
    api_v0::GETRANDOM => sys::sys_getrandom,
    api_v0::MEMFD_CREATE => sys::sys_memfd_create,
    api_v0::GETITIMER => sys::sys_getitimer,
    api_v0::SETITIMER => sys::sys_setitimer,
    api_v0::GETRLIMIT => sys::sys_getrlimit,
    api_v0::GETRUSAGE => sys::sys_getrusage,
    api_v0::SETRLIMIT => sys::sys_setrlimit,
    api_v0::UMASK => sys::sys_umask,
    api_v0::PRLIMIT64 => sys::sys_prlimit64,
    api_v0::SOCKET => sys::sys_socket,
    api_v0::SOCKETPAIR => sys::sys_socketpair,
    api_v0::BIND => sys::sys_bind,
    api_v0::LISTEN => sys::sys_listen,
    api_v0::ACCEPT => sys::sys_accept,
    api_v0::ACCEPT4 => sys::sys_accept4,
    api_v0::CONNECT => sys::sys_connect,
    api_v0::GETSOCKNAME => sys::sys_getsockname,
    api_v0::GETPEERNAME => sys::sys_getpeername,
    api_v0::SENDTO => sys::sys_sendto,
    api_v0::RECVFROM => sys::sys_recvfrom,
    api_v0::SENDMSG => sys::sys_sendmsg,
    api_v0::RECVMSG => sys::sys_recvmsg,
    api_v0::SENDMMSG => sys::sys_sendmmsg,
    api_v0::RECVMMSG => sys::sys_recvmmsg,
    api_v0::SETSOCKOPT => sys::sys_setsockopt,
    api_v0::GETSOCKOPT => sys::sys_getsockopt,
    api_v0::SHUTDOWN => sys::sys_shutdown,
    api_v0::PPOLL => sys::sys_ppoll,
    api_v0::SIGNALFD4 => sys::sys_signalfd4,
    api_v0::PSELECT6 => sys::sys_pselect6,
    api_v0::EPOLL_CREATE1 => sys::sys_epoll_create1,
    api_v0::EPOLL_CTL => sys::sys_epoll_ctl,
    api_v0::EPOLL_PWAIT => sys::sys_epoll_pwait,
    api_v0::EPOLL_PWAIT2 => sys::sys_epoll_pwait2,
    api_v0::FSTATAT => sys::sys_fstatat,
    api_v0::STATX => sys::sys_statx,
    api_v0::SCHED_SETATTR => sys::sys_sched_setattr,
    api_v0::SCHED_GETATTR => sys::sys_sched_getattr,
    api_v0::SCHED_RR_GET_INTERVAL => sys::sys_sched_rr_get_interval,
    api_v0::FACESSAT2 => sys::sys_faccessat2,
    api_v0::ADJTIMEX => sys::sys_adjtimex,
    api_v0::CLOCK_ADJTIME => sys::sys_clock_adjtime,
    api_v0::ACCT => sys::sys_acct,
    api_v0::CLOSE_RANGE => sys::sys_close_range,
    api_v0::SETGROUPS => sys::sys_setgroups,
    api_v0::FSTATFS => sys::sys_fstatfs,
    api_v0::LINKAT => sys::sys_linkat,
    api_v0::MKNODAT => sys::sys_mknodat,
    api_v0::SETXATTR => sys::sys_setxattr,
    api_v0::LSETXATTR => sys::sys_lsetxattr,
    api_v0::FSETXATTR => sys::sys_fsetxattr,
    api_v0::GETXATTR => sys::sys_getxattr,
    api_v0::LGETXATTR => sys::sys_lgetxattr,
    api_v0::FGETXATTR => sys::sys_fgetxattr,
    api_v0::LISTXATTR => sys::sys_listxattr,
    api_v0::LLISTXATTR => sys::sys_llistxattr,
    api_v0::FLISTXATTR => sys::sys_flistxattr,
    api_v0::REMOVEXATTR => sys::sys_removexattr,
    api_v0::LREMOVEXATTR => sys::sys_lremovexattr,
    api_v0::FREMOVEXATTR => sys::sys_fremovexattr,
    api_v0::INOTIFY_INIT1 => sys::sys_inotify_init1,
    api_v0::INOTIFY_ADD_WATCH => sys::sys_inotify_add_watch,
    api_v0::INOTIFY_RM_WATCH => sys::sys_inotify_rm_watch,
}

noarg_syscalls! {
    yield_ => sys::sys_yield,
    getpid => sys::sys_getpid,
    getppid => sys::sys_getppid,
    gettid => sys::sys_gettid,
    getuid => sys::sys_getuid,
    geteuid => sys::sys_geteuid,
    getgid => sys::sys_getgid,
    getegid => sys::sys_getegid,
    setsid => sys::sys_setsid,
}

#[inline]
fn exit_dispatch(args : SyscallArgs) -> isize { sys::sys_exit(args.arg(0) as isize) }

#[inline]
fn exit_group_dispatch(args : SyscallArgs) -> isize { sys::sys_exit_group(args.arg(0) as isize) }

#[inline]
fn brk_dispatch(args : SyscallArgs) -> isize { sys::sys_brk(args.arg(0)).0 }

#[cfg(target_arch = "riscv64")]
#[inline]
fn riscv_hwprobe_dispatch(args : SyscallArgs) -> isize { sys::sys_riscv_hwprobe(args).0 }

#[cfg(not(target_arch = "riscv64"))]
#[inline]
fn riscv_hwprobe_dispatch(_args : SyscallArgs) -> isize { sys_enosys(_args) }

#[cfg(target_arch = "riscv64")]
#[inline]
fn riscv_flush_icache_dispatch(args : SyscallArgs) -> isize { sys::sys_riscv_flush_icache(args).0 }

#[cfg(not(target_arch = "riscv64"))]
#[inline]
fn riscv_flush_icache_dispatch(_args : SyscallArgs) -> isize { sys_enosys(_args) }

const SPECIAL_SYSCALL_TABLE : [Option<SpecialHandler>; SYSCALL_TABLE_SIZE] = {
    let mut table = [None; SYSCALL_TABLE_SIZE];

    table[api_v0::YIELD] = Some(yield_ as SpecialHandler);
    table[api_v0::GETPID] = Some(getpid as SpecialHandler);
    table[api_v0::GETPPID] = Some(getppid as SpecialHandler);
    table[api_v0::GETTID] = Some(gettid as SpecialHandler);
    table[api_v0::GETUID] = Some(getuid as SpecialHandler);
    table[api_v0::GETEUID] = Some(geteuid as SpecialHandler);
    table[api_v0::GETGID] = Some(getgid as SpecialHandler);
    table[api_v0::GETEGID] = Some(getegid as SpecialHandler);
    table[api_v0::SETSID] = Some(setsid as SpecialHandler);
    table[api_v0::EXIT] = Some(exit_dispatch as SpecialHandler);
    table[api_v0::EXIT_GROUP] = Some(exit_group_dispatch as SpecialHandler);
    table[api_v0::BRK] = Some(brk_dispatch as SpecialHandler);
    table[api_v0::RISCV_HWPROBE] = Some(riscv_hwprobe_dispatch as SpecialHandler);
    table[api_v0::RISCV_FLUSH_ICACHE] = Some(riscv_flush_icache_dispatch as SpecialHandler);

    table
};

/// 按裸 syscall 号分发；未命中时走 ENOSYS。
#[inline]
pub fn dispatch_syscall_by_nr(syscall_nr : usize, syscall_args : SyscallArgs) -> isize {
    if syscall_nr < SYSCALL_TABLE_SIZE {
        if let Some(handler) = ARG_SYSCALL_TABLE[syscall_nr] {
            return handler(syscall_args).0;
        }
        if let Some(handler) = SPECIAL_SYSCALL_TABLE[syscall_nr] {
            return handler(syscall_args);
        }
    }
    sys_enosys(syscall_args)
}

/// EINTR 后可重启 syscall。
#[inline]
pub fn is_restartable_syscall_nr(syscall_nr : usize) -> bool {
    syscall_nr == api_v0::READ ||
    syscall_nr == api_v0::READV ||
    syscall_nr == api_v0::WRITE ||
    syscall_nr == api_v0::WRITEV ||
    syscall_nr == api_v0::WAITPID ||
    syscall_nr == api_v0::WAITID ||
    syscall_nr == api_v0::ACCEPT4 ||
    syscall_nr == api_v0::CONNECT ||
    syscall_nr == api_v0::SENDTO ||
    syscall_nr == api_v0::RECVFROM ||
    syscall_nr == api_v0::SENDMSG ||
    syscall_nr == api_v0::RECVMSG ||
    syscall_nr == api_v0::SENDMMSG ||
    syscall_nr == api_v0::RECVMMSG ||
    syscall_nr == api_v0::MSGRCV ||
    syscall_nr == api_v0::MSGSND ||
    syscall_nr == api_v0::SEMOP ||
    syscall_nr == api_v0::SEMTIMEDOP
}
