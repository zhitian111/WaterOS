#![no_std]
//! Kernel syscall implementation.
//!
//! This crate owns the concrete `sys_*` behavior and implements the dispatcher
//! trait declared by `wateros-syscall-api-v0`.

extern crate alloc;

use abi::syscall_args::SyscallArgs;
use abi::syscall_number::ActiveSyscallNumberTable;

mod linux_stat;
mod mm_util;
mod poll_engine;
mod socket_fd;
mod sys;
mod user_copy;
mod vfs_util;

/// Trap / exception return path syscall dispatch entry.
#[inline]
pub fn dispatch_syscall_from_trap(syscall_nr: usize, syscall_args: SyscallArgs) -> isize {
    <KernelSyscallDispatcher as api_v0::SyscallDispatcher>::dispatch_syscall_from_trap(
        syscall_nr,
        syscall_args,
    )
}

#[inline]
pub fn timer_tick(interrupted_user: bool) {
    sys::timer_tick(interrupted_user);
}

#[inline]
pub fn deliver_pending_signal(
    frame: *mut u8,
    restart: Option<(usize, SyscallArgs)>,
) -> isize {
    match sys::deliver_pending_signal(frame, restart) {
        Ok(false) => 0,
        Ok(true) => 1,
        Err(_) => -1,
    }
}

#[inline]
pub fn restore_signal_frame(frame: *mut u8) -> bool {
    sys::restore_signal_frame(frame).is_ok()
}

pub fn raise_current_signal(signal: usize) -> bool {
    sys::raise_current_thread(signal).is_ok()
}

pub fn drop_reaped_task_runtime_resources(task_id: usize, aspace: usize) {
    sys::drop_reaped_task_runtime_resources(task_id, aspace);
}

pub fn record_user_page_fault_handled() {
    sys::record_user_page_fault_handled();
}

pub fn log_thread_bringup_stats_summary() {
    sys::log_thread_bringup_stats_summary();
}

/// Kernel syscall implementation selected by the aggregate crate.
pub struct KernelSyscallDispatcher;

const SYS_STATX: usize = 291;
const SYS_FSTATAT: usize = 79;
const SYS_SCHED_SETATTR: usize = 274;
const SYS_SCHED_GETATTR: usize = 275;
const SYS_FACCESSAT2: usize = 439;

impl api_v0::SyscallDispatcher for KernelSyscallDispatcher {
    type NumberTable = ActiveSyscallNumberTable;

    #[inline]
    fn dispatch_yield(_args: SyscallArgs) -> isize {
        sys::sys_yield().0
    }

    #[inline]
    fn dispatch_sched_setparam(args: SyscallArgs) -> isize {
        sys::sys_sched_setparam(args).0
    }

    #[inline]
    fn dispatch_sched_setscheduler(args: SyscallArgs) -> isize {
        sys::sys_sched_setscheduler(args).0
    }

    #[inline]
    fn dispatch_sched_getparam(args: SyscallArgs) -> isize {
        sys::sys_sched_getparam(args).0
    }

    #[inline]
    fn dispatch_sched_getscheduler(args: SyscallArgs) -> isize {
        sys::sys_sched_getscheduler(args).0
    }

    #[inline]
    fn dispatch_sched_setaffinity(args: SyscallArgs) -> isize {
        sys::sys_sched_setaffinity(args).0
    }

    #[inline]
    fn dispatch_sched_getaffinity(args: SyscallArgs) -> isize {
        sys::sys_sched_getaffinity(args).0
    }

    #[inline]
    fn dispatch_sched_get_priority_max(args: SyscallArgs) -> isize {
        sys::sys_sched_get_priority_max(args).0
    }

    #[inline]
    fn dispatch_sched_get_priority_min(args: SyscallArgs) -> isize {
        sys::sys_sched_get_priority_min(args).0
    }

    #[inline]
    fn dispatch_clone(args: SyscallArgs) -> isize {
        sys::sys_clone(args).0
    }

    #[inline]
    fn dispatch_clone3(args: SyscallArgs) -> isize {
        sys::sys_clone3(args).0
    }

    #[inline]
    fn dispatch_exit(args: SyscallArgs) -> isize {
        sys::sys_exit(args.arg(0) as isize)
    }

    #[inline]
    fn dispatch_exit_group(args: SyscallArgs) -> isize {
        sys::sys_exit_group(args.arg(0) as isize)
    }

    #[inline]
    fn dispatch_read(args: SyscallArgs) -> isize {
        sys::sys_read(args).0
    }

    #[inline]
    fn dispatch_readv(args: SyscallArgs) -> isize {
        sys::sys_readv(args).0
    }

    #[inline]
    fn dispatch_write(args: SyscallArgs) -> isize {
        sys::sys_write(args).0
    }

    #[inline]
    fn dispatch_writev(args: SyscallArgs) -> isize {
        sys::sys_writev(args).0
    }

    #[inline]
    fn dispatch_pread64(args: SyscallArgs) -> isize {
        sys::sys_pread64(args).0
    }

    #[inline]
    fn dispatch_pwrite64(args: SyscallArgs) -> isize {
        sys::sys_pwrite64(args).0
    }

    #[inline]
    fn dispatch_preadv(args: SyscallArgs) -> isize {
        sys::sys_preadv(args).0
    }

    #[inline]
    fn dispatch_pwritev(args: SyscallArgs) -> isize {
        sys::sys_pwritev(args).0
    }

    #[inline]
    fn dispatch_sendfile(args: SyscallArgs) -> isize {
        sys::sys_sendfile(args).0
    }

    #[inline]
    fn dispatch_readlinkat(args: SyscallArgs) -> isize {
        sys::sys_readlinkat(args).0
    }

    #[inline]
    fn dispatch_faccessat(args: SyscallArgs) -> isize {
        sys::sys_faccessat(args).0
    }

    #[inline]
    fn dispatch_fchmodat(args: SyscallArgs) -> isize {
        sys::sys_fchmodat(args).0
    }

    #[inline]
    fn dispatch_fchownat(args: SyscallArgs) -> isize {
        sys::sys_fchownat(args).0
    }

    #[inline]
    fn dispatch_statfs(args: SyscallArgs) -> isize {
        sys::sys_statfs(args).0
    }

    #[inline]
    fn dispatch_sync(args: SyscallArgs) -> isize {
        sys::sys_sync(args).0
    }

    #[inline]
    fn dispatch_fsync(args: SyscallArgs) -> isize {
        sys::sys_fsync(args).0
    }

    #[inline]
    fn dispatch_fdatasync(args: SyscallArgs) -> isize {
        sys::sys_fdatasync(args).0
    }

    #[inline]
    fn dispatch_ftruncate(args: SyscallArgs) -> isize {
        sys::sys_ftruncate(args).0
    }

    #[inline]
    fn dispatch_fallocate(args: SyscallArgs) -> isize {
        sys::sys_fallocate(args).0
    }

    #[inline]
    fn dispatch_openat(args: SyscallArgs) -> isize {
        sys::sys_openat(args).0
    }

    #[inline]
    fn dispatch_close(args: SyscallArgs) -> isize {
        sys::sys_close(args).0
    }

    #[inline]
    fn dispatch_fstat(args: SyscallArgs) -> isize {
        sys::sys_fstat(args).0
    }

    #[inline]
    fn dispatch_lseek(args: SyscallArgs) -> isize {
        sys::sys_lseek(args).0
    }

    #[inline]
    fn dispatch_dup(_args: SyscallArgs) -> isize {
        sys::sys_dup(_args).0
    }

    #[inline]
    fn dispatch_dup3(_args: SyscallArgs) -> isize {
        sys::sys_dup3(_args).0
    }

    #[inline]
    fn dispatch_pipe2(args: SyscallArgs) -> isize {
        sys::sys_pipe2(args).0
    }

    #[inline]
    fn dispatch_brk(args: SyscallArgs) -> isize {
        sys::sys_brk(args.arg(0)).0
    }

    #[inline]
    fn dispatch_mmap(args: SyscallArgs) -> isize {
        sys::sys_mmap(args).0
    }

    #[inline]
    fn dispatch_munmap(args: SyscallArgs) -> isize {
        sys::sys_munmap(args).0
    }

    #[inline]
    fn dispatch_msync(args: SyscallArgs) -> isize {
        sys::sys_msync(args).0
    }

    #[inline]
    fn dispatch_mprotect(args: SyscallArgs) -> isize {
        sys::sys_mprotect(args).0
    }

    #[inline]
    fn dispatch_mremap(args: SyscallArgs) -> isize {
        sys::sys_mremap(args).0
    }

    #[inline]
    fn dispatch_madvise(args: SyscallArgs) -> isize {
        sys::sys_madvise(args).0
    }

    #[inline]
    fn dispatch_mlock(args: SyscallArgs) -> isize {
        sys::sys_mlock(args).0
    }

    #[inline]
    fn dispatch_munlock(args: SyscallArgs) -> isize {
        sys::sys_munlock(args).0
    }

    #[inline]
    fn dispatch_mlockall(args: SyscallArgs) -> isize {
        sys::sys_mlockall(args).0
    }

    #[inline]
    fn dispatch_munlockall(args: SyscallArgs) -> isize {
        sys::sys_munlockall(args).0
    }

    #[inline]
    fn dispatch_getmempolicy(args: SyscallArgs) -> isize {
        sys::sys_get_mempolicy(args).0
    }

    #[inline]
    fn dispatch_shmget(args: SyscallArgs) -> isize {
        sys::sys_shmget(args).0
    }

    #[inline]
    fn dispatch_shmctl(args: SyscallArgs) -> isize {
        sys::sys_shmctl(args).0
    }

    #[inline]
    fn dispatch_shmat(args: SyscallArgs) -> isize {
        sys::sys_shmat(args).0
    }

    #[inline]
    fn dispatch_shmdt(args: SyscallArgs) -> isize {
        sys::sys_shmdt(args).0
    }

    #[inline]
    fn dispatch_get_time(args: SyscallArgs) -> isize {
        sys::sys_gettimeofday(args).0
    }

    #[inline]
    fn dispatch_clock_gettime(args: SyscallArgs) -> isize {
        sys::sys_clock_gettime(args).0
    }

    #[inline]
    fn dispatch_clock_settime(args: SyscallArgs) -> isize {
        sys::sys_clock_settime(args).0
    }

    #[inline]
    fn dispatch_clock_getres(args: SyscallArgs) -> isize {
        sys::sys_clock_getres(args).0
    }

    #[inline]
    fn dispatch_clock_nanosleep(args: SyscallArgs) -> isize {
        sys::sys_clock_nanosleep(args).0
    }

    #[inline]
    fn dispatch_getpid(_args: SyscallArgs) -> isize {
        sys::sys_getpid().0
    }

    #[inline]
    fn dispatch_getppid(_args: SyscallArgs) -> isize {
        sys::sys_getppid().0
    }

    #[inline]
    fn dispatch_gettid(_args: SyscallArgs) -> isize {
        sys::sys_gettid().0
    }

    #[inline]
    fn dispatch_getuid(_args: SyscallArgs) -> isize {
        sys::sys_getuid().0
    }

    #[inline]
    fn dispatch_geteuid(_args: SyscallArgs) -> isize {
        sys::sys_geteuid().0
    }

    #[inline]
    fn dispatch_getgid(_args: SyscallArgs) -> isize {
        sys::sys_getgid().0
    }

    #[inline]
    fn dispatch_getegid(_args: SyscallArgs) -> isize {
        sys::sys_getegid().0
    }

    #[inline]
    fn dispatch_setsid(_args: SyscallArgs) -> isize {
        sys::sys_setsid().0
    }

    #[inline]
    fn dispatch_getgroups(args: SyscallArgs) -> isize {
        sys::sys_getgroups(args).0
    }

    #[inline]
    fn dispatch_sysinfo(args: SyscallArgs) -> isize {
        sys::sys_sysinfo(args).0
    }

    #[inline]
    fn dispatch_setuid(args: SyscallArgs) -> isize {
        sys::sys_setuid(args).0
    }

    #[inline]
    fn dispatch_setgid(args: SyscallArgs) -> isize {
        sys::sys_setgid(args).0
    }

    #[inline]
    fn dispatch_setreuid(args: SyscallArgs) -> isize {
        sys::sys_setreuid(args).0
    }

    #[inline]
    fn dispatch_setregid(args: SyscallArgs) -> isize {
        sys::sys_setregid(args).0
    }

    #[inline]
    fn dispatch_setresuid(args: SyscallArgs) -> isize {
        sys::sys_setresuid(args).0
    }

    #[inline]
    fn dispatch_setresgid(args: SyscallArgs) -> isize {
        sys::sys_setresgid(args).0
    }

    #[inline]
    fn dispatch_futex(args: SyscallArgs) -> isize {
        sys::sys_futex(args).0
    }

    #[inline]
    fn dispatch_fcntl(args: SyscallArgs) -> isize {
        sys::sys_fcntl(args).0
    }

    #[inline]
    fn dispatch_ioctl(args: SyscallArgs) -> isize {
        sys::sys_ioctl(args).0
    }

    #[inline]
    fn dispatch_execve(args: SyscallArgs) -> isize {
        sys::sys_execve(args).0
    }

    #[inline]
    fn dispatch_waitpid(args: SyscallArgs) -> isize {
        sys::sys_waitpid(args).0
    }

    #[inline]
    fn dispatch_kill(args: SyscallArgs) -> isize {
        sys::sys_kill(args).0
    }

    #[inline]
    fn dispatch_nanosleep(args: SyscallArgs) -> isize {
        sys::sys_nanosleep(args).0
    }

    #[inline]
    fn dispatch_times(args: SyscallArgs) -> isize {
        sys::sys_times(args).0
    }

    #[inline]
    fn dispatch_setpgid(args: SyscallArgs) -> isize {
        sys::sys_setpgid(args).0
    }

    #[inline]
    fn dispatch_getcwd(args: SyscallArgs) -> isize {
        sys::sys_getcwd(args).0
    }

    #[inline]
    fn dispatch_chdir(args: SyscallArgs) -> isize {
        sys::sys_chdir(args).0
    }

    #[inline]
    fn dispatch_mkdirat(args: SyscallArgs) -> isize {
        sys::sys_mkdirat(args).0
    }

    #[inline]
    fn dispatch_getdents64(args: SyscallArgs) -> isize {
        sys::sys_getdents64(args).0
    }

    #[inline]
    fn dispatch_unlinkat(args: SyscallArgs) -> isize {
        sys::sys_unlinkat(args).0
    }

    #[inline]
    fn dispatch_renameat2(args: SyscallArgs) -> isize {
        sys::sys_renameat2(args).0
    }

    #[inline]
    fn dispatch_utimensat(args: SyscallArgs) -> isize {
        sys::sys_utimensat(args).0
    }

    #[inline]
    fn dispatch_mount(args: SyscallArgs) -> isize {
        sys::sys_mount(args).0
    }

    #[inline]
    fn dispatch_umount2(args: SyscallArgs) -> isize {
        sys::sys_umount2(args).0
    }

    #[inline]
    fn dispatch_uname(args: SyscallArgs) -> isize {
        sys::sys_uname(args).0
    }

    #[inline]
    fn dispatch_syslog(args: SyscallArgs) -> isize {
        sys::sys_syslog(args).0
    }

    #[inline]
    fn dispatch_prctl(args: SyscallArgs) -> isize {
        sys::sys_prctl(args).0
    }

    #[inline]
    fn dispatch_getrlimit(args: SyscallArgs) -> isize {
        sys::sys_getrlimit(args).0
    }

    #[inline]
    fn dispatch_getrusage(args: SyscallArgs) -> isize {
        sys::sys_getrusage(args).0
    }

    #[inline]
    fn dispatch_setitimer(args: SyscallArgs) -> isize {
        sys::sys_setitimer(args).0
    }

    #[inline]
    fn dispatch_getitimer(args: SyscallArgs) -> isize {
        sys::sys_getitimer(args).0
    }

    #[inline]
    fn dispatch_rt_sigpending(args: SyscallArgs) -> isize {
        sys::sys_rt_sigpending(args).0
    }

    #[inline]
    fn dispatch_rt_sigsuspend(args: SyscallArgs) -> isize {
        sys::sys_rt_sigsuspend(args).0
    }

    #[inline]
    fn dispatch_tkill(args: SyscallArgs) -> isize {
        sys::sys_tkill(args).0
    }

    #[inline]
    fn dispatch_tgkill(args: SyscallArgs) -> isize {
        sys::sys_tgkill(args).0
    }

    #[inline]
    fn dispatch_setrlimit(args: SyscallArgs) -> isize {
        sys::sys_setrlimit(args).0
    }

    #[inline]
    fn dispatch_umask(args: SyscallArgs) -> isize {
        sys::sys_umask(args).0
    }

    #[inline]
    fn dispatch_prlimit64(args: SyscallArgs) -> isize {
        sys::sys_prlimit64(args).0
    }

    #[inline]
    fn dispatch_set_tid_address(args: SyscallArgs) -> isize {
        sys::sys_set_tid_address(args).0
    }

    #[inline]
    fn dispatch_set_robust_list(args: SyscallArgs) -> isize {
        sys::sys_set_robust_list(args).0
    }

    #[inline]
    fn dispatch_get_robust_list(args: SyscallArgs) -> isize {
        sys::sys_get_robust_list(args).0
    }

    #[inline]
    fn dispatch_getrandom(args: SyscallArgs) -> isize {
        sys::sys_getrandom(args).0
    }

    #[inline]
    fn dispatch_rt_sigaction(args: SyscallArgs) -> isize {
        sys::sys_rt_sigaction(args).0
    }

    #[inline]
    fn dispatch_rt_sigprocmask(args: SyscallArgs) -> isize {
        sys::sys_rt_sigprocmask(args).0
    }

    #[inline]
    fn dispatch_rt_sigtimedwait(args: SyscallArgs) -> isize {
        sys::sys_rt_sigtimedwait(args).0
    }

    #[inline]
    fn dispatch_unsupported(
        kind: api_v0::SyscallKind,
        syscall_nr: usize,
        args: SyscallArgs,
    ) -> isize {
        api_v0::unsupported::syscall_unsupported_decoded(kind, syscall_nr, args);
    }

    #[inline]
    fn dispatch_unknown(syscall_nr: usize, args: SyscallArgs) -> isize {
        if syscall_nr == SYS_FSTATAT {
            return sys::sys_fstatat(args).0;
        }
        if syscall_nr == SYS_STATX {
            return sys::sys_statx(args).0;
        }
        if syscall_nr == SYS_SCHED_SETATTR {
            return sys::sys_sched_setattr(args).0;
        }
        if syscall_nr == SYS_SCHED_GETATTR {
            return sys::sys_sched_getattr(args).0;
        }
        if syscall_nr == SYS_FACCESSAT2 {
            return sys::sys_faccessat2(args).0;
        }
        let _ = args;
        abi::user_ret::UserRet::from_error(abi::errno::ErrNo::ENOSYS).0
    }

    // ——— socket / 网络 ———

    #[inline]
    fn dispatch_socket(args: SyscallArgs) -> isize {
        sys::sys_socket(args).0
    }

    #[inline]
    fn dispatch_socketpair(args: SyscallArgs) -> isize {
        sys::sys_socketpair(args).0
    }

    #[inline]
    fn dispatch_bind(args: SyscallArgs) -> isize {
        sys::sys_bind(args).0
    }

    #[inline]
    fn dispatch_listen(args: SyscallArgs) -> isize {
        sys::sys_listen(args).0
    }

    #[inline]
    fn dispatch_accept4(args: SyscallArgs) -> isize {
        sys::sys_accept4(args).0
    }

    #[inline]
    fn dispatch_accept(args: SyscallArgs) -> isize {
        sys::sys_accept(args).0
    }

    #[inline]
    fn dispatch_connect(args: SyscallArgs) -> isize {
        sys::sys_connect(args).0
    }

    #[inline]
    fn dispatch_getsockname(args: SyscallArgs) -> isize {
        sys::sys_getsockname(args).0
    }

    #[inline]
    fn dispatch_getpeername(args: SyscallArgs) -> isize {
        sys::sys_getpeername(args).0
    }

    #[inline]
    fn dispatch_sendto(args: SyscallArgs) -> isize {
        sys::sys_sendto(args).0
    }

    #[inline]
    fn dispatch_recvfrom(args: SyscallArgs) -> isize {
        sys::sys_recvfrom(args).0
    }

    #[inline]
    fn dispatch_sendmsg(args: SyscallArgs) -> isize {
        sys::sys_sendmsg(args).0
    }

    #[inline]
    fn dispatch_recvmsg(args: SyscallArgs) -> isize {
        sys::sys_recvmsg(args).0
    }

    #[inline]
    fn dispatch_setsockopt(args: SyscallArgs) -> isize {
        sys::sys_setsockopt(args).0
    }

    #[inline]
    fn dispatch_getsockopt(args: SyscallArgs) -> isize {
        sys::sys_getsockopt(args).0
    }

    #[inline]
    fn dispatch_shutdown(args: SyscallArgs) -> isize {
        sys::sys_shutdown(args).0
    }

    #[inline]
    fn dispatch_ppoll(args: SyscallArgs) -> isize {
        sys::sys_ppoll(args).0
    }

    #[inline]
    fn dispatch_pselect6(args: SyscallArgs) -> isize {
        sys::sys_pselect6(args).0
    }

    #[inline]
    fn dispatch_select(args: SyscallArgs) -> isize {
        sys::sys_select(args).0
    }

    #[inline]
    fn dispatch_poll(args: SyscallArgs) -> isize {
        sys::sys_poll(args).0
    }
}
