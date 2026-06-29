// Syscall 号单次 match 分发（H-3）：替代 `SyscallKind::decode` + 巨型 match。
// 本模块代码由AI完成

/// 按裸 syscall 号分发；未命中时走旁路号与 ENOSYS。
#[inline]
// 本方法代码由AI完成
pub fn dispatch_syscall_by_nr(syscall_nr: usize, syscall_args: SyscallArgs) -> isize {
    use api_v0::SyscallDispatcher;
    match syscall_nr {
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::YIELD.raw() =>
            KernelSyscallDispatcher::dispatch_yield(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SCHED_SETPARAM.raw() =>
            KernelSyscallDispatcher::dispatch_sched_setparam(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SCHED_SETSCHEDULER.raw() =>
            KernelSyscallDispatcher::dispatch_sched_setscheduler(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SCHED_GETSCHEDULER.raw() =>
            KernelSyscallDispatcher::dispatch_sched_getscheduler(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SCHED_GETPARAM.raw() =>
            KernelSyscallDispatcher::dispatch_sched_getparam(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SCHED_SETAFFINITY.raw() =>
            KernelSyscallDispatcher::dispatch_sched_setaffinity(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SCHED_GETAFFINITY.raw() =>
            KernelSyscallDispatcher::dispatch_sched_getaffinity(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SCHED_GET_PRIORITY_MAX.raw() =>
            KernelSyscallDispatcher::dispatch_sched_get_priority_max(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SCHED_GET_PRIORITY_MIN.raw() =>
            KernelSyscallDispatcher::dispatch_sched_get_priority_min(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::EXIT.raw() =>
            KernelSyscallDispatcher::dispatch_exit(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::EXIT_GROUP.raw() =>
            KernelSyscallDispatcher::dispatch_exit_group(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::READ.raw() =>
            KernelSyscallDispatcher::dispatch_read(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::READV.raw() =>
            KernelSyscallDispatcher::dispatch_readv(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::WRITE.raw() =>
            KernelSyscallDispatcher::dispatch_write(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::WRITEV.raw() =>
            KernelSyscallDispatcher::dispatch_writev(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::PREAD64.raw() =>
            KernelSyscallDispatcher::dispatch_pread64(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::PWRITE64.raw() =>
            KernelSyscallDispatcher::dispatch_pwrite64(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::PREADV.raw() =>
            KernelSyscallDispatcher::dispatch_preadv(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::PWRITEV.raw() =>
            KernelSyscallDispatcher::dispatch_pwritev(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SENDFILE.raw() =>
            KernelSyscallDispatcher::dispatch_sendfile(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::READLINKAT.raw() =>
            KernelSyscallDispatcher::dispatch_readlinkat(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::FACCESSAT.raw() =>
            KernelSyscallDispatcher::dispatch_faccessat(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::FCHMOD.raw() =>
            KernelSyscallDispatcher::dispatch_fchmod(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::FCHMODAT.raw() =>
            KernelSyscallDispatcher::dispatch_fchmodat(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::FCHOWN.raw() =>
            KernelSyscallDispatcher::dispatch_fchown(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::FCHOWNAT.raw() =>
            KernelSyscallDispatcher::dispatch_fchownat(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::STATFS.raw() =>
            KernelSyscallDispatcher::dispatch_statfs(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SYNC.raw() =>
            KernelSyscallDispatcher::dispatch_sync(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::FSYNC.raw() =>
            KernelSyscallDispatcher::dispatch_fsync(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::FDATASYNC.raw() =>
            KernelSyscallDispatcher::dispatch_fdatasync(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::TRUNCATE.raw() =>
            KernelSyscallDispatcher::dispatch_truncate(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::FTRUNCATE.raw() =>
            KernelSyscallDispatcher::dispatch_ftruncate(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::FALLOCATE.raw() =>
            KernelSyscallDispatcher::dispatch_fallocate(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::OPENAT.raw() =>
            KernelSyscallDispatcher::dispatch_openat(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::CLOSE.raw() =>
            KernelSyscallDispatcher::dispatch_close(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::FSTAT.raw() =>
            KernelSyscallDispatcher::dispatch_fstat(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::LSEEK.raw() =>
            KernelSyscallDispatcher::dispatch_lseek(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::DUP.raw() =>
            KernelSyscallDispatcher::dispatch_dup(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::DUP3.raw() =>
            KernelSyscallDispatcher::dispatch_dup3(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::PIPE2.raw() =>
            KernelSyscallDispatcher::dispatch_pipe2(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::IOCTL.raw() =>
            KernelSyscallDispatcher::dispatch_ioctl(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::FCNTL.raw() =>
            KernelSyscallDispatcher::dispatch_fcntl(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::FLOCK.raw() =>
            KernelSyscallDispatcher::dispatch_flock(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GETDENTS64.raw() =>
            KernelSyscallDispatcher::dispatch_getdents64(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::MKDIRAT.raw() =>
            KernelSyscallDispatcher::dispatch_mkdirat(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SYMLINKAT.raw() =>
            KernelSyscallDispatcher::dispatch_symlinkat(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::UNLINKAT.raw() =>
            KernelSyscallDispatcher::dispatch_unlinkat(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::RENAMEAT.raw() =>
            KernelSyscallDispatcher::dispatch_renameat(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::RENAMEAT2.raw() =>
            KernelSyscallDispatcher::dispatch_renameat2(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::UTIMENSAT.raw() =>
            KernelSyscallDispatcher::dispatch_utimensat(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::MOUNT.raw() =>
            KernelSyscallDispatcher::dispatch_mount(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::UMOUNT2.raw() =>
            KernelSyscallDispatcher::dispatch_umount2(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::BRK.raw() =>
            KernelSyscallDispatcher::dispatch_brk(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::FORK.raw() =>
            KernelSyscallDispatcher::dispatch_clone(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::CLONE3.raw() =>
            KernelSyscallDispatcher::dispatch_clone3(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::UNSHARE.raw() =>
            KernelSyscallDispatcher::dispatch_unshare(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::EXEC.raw() =>
            KernelSyscallDispatcher::dispatch_execve(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::MMAP.raw() =>
            KernelSyscallDispatcher::dispatch_mmap(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::MUNMAP.raw() =>
            KernelSyscallDispatcher::dispatch_munmap(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::MSYNC.raw() =>
            KernelSyscallDispatcher::dispatch_msync(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::MPROTECT.raw() =>
            KernelSyscallDispatcher::dispatch_mprotect(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::MREMAP.raw() =>
            KernelSyscallDispatcher::dispatch_mremap(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::MADVISE.raw() =>
            KernelSyscallDispatcher::dispatch_madvise(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::MLOCK.raw() =>
            KernelSyscallDispatcher::dispatch_mlock(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::MUNLOCK.raw() =>
            KernelSyscallDispatcher::dispatch_munlock(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::MLOCKALL.raw() =>
            KernelSyscallDispatcher::dispatch_mlockall(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::MUNLOCKALL.raw() =>
            KernelSyscallDispatcher::dispatch_munlockall(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GET_MEMPOLICY.raw() =>
            KernelSyscallDispatcher::dispatch_getmempolicy(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SHMGET.raw() =>
            KernelSyscallDispatcher::dispatch_shmget(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SHMCTL.raw() =>
            KernelSyscallDispatcher::dispatch_shmctl(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SHMAT.raw() =>
            KernelSyscallDispatcher::dispatch_shmat(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SHMDT.raw() =>
            KernelSyscallDispatcher::dispatch_shmdt(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GET_TIME.raw() =>
            KernelSyscallDispatcher::dispatch_get_time(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::CLOCK_SETTIME.raw() =>
            KernelSyscallDispatcher::dispatch_clock_settime(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::CLOCK_GETTIME.raw() =>
            KernelSyscallDispatcher::dispatch_clock_gettime(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::CLOCK_GETRES.raw() =>
            KernelSyscallDispatcher::dispatch_clock_getres(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::CLOCK_NANOSLEEP.raw() =>
            KernelSyscallDispatcher::dispatch_clock_nanosleep(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GETPID.raw() =>
            KernelSyscallDispatcher::dispatch_getpid(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GETPPID.raw() =>
            KernelSyscallDispatcher::dispatch_getppid(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GETTID.raw() =>
            KernelSyscallDispatcher::dispatch_gettid(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GETUID.raw() =>
            KernelSyscallDispatcher::dispatch_getuid(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GETEUID.raw() =>
            KernelSyscallDispatcher::dispatch_geteuid(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GETGID.raw() =>
            KernelSyscallDispatcher::dispatch_getgid(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GETEGID.raw() =>
            KernelSyscallDispatcher::dispatch_getegid(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SETSID.raw() =>
            KernelSyscallDispatcher::dispatch_setsid(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GETGROUPS.raw() =>
            KernelSyscallDispatcher::dispatch_getgroups(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SYSINFO.raw() =>
            KernelSyscallDispatcher::dispatch_sysinfo(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SETUID.raw() =>
            KernelSyscallDispatcher::dispatch_setuid(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SETGID.raw() =>
            KernelSyscallDispatcher::dispatch_setgid(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SETREUID.raw() =>
            KernelSyscallDispatcher::dispatch_setreuid(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SETREGID.raw() =>
            KernelSyscallDispatcher::dispatch_setregid(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SETRESUID.raw() =>
            KernelSyscallDispatcher::dispatch_setresuid(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SETRESGID.raw() =>
            KernelSyscallDispatcher::dispatch_setresgid(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GETRESUID.raw() =>
            KernelSyscallDispatcher::dispatch_getresuid(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GETRESGID.raw() =>
            KernelSyscallDispatcher::dispatch_getresgid(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::TIMES.raw() =>
            KernelSyscallDispatcher::dispatch_times(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SETPGID.raw() =>
            KernelSyscallDispatcher::dispatch_setpgid(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GETPGID.raw() =>
            KernelSyscallDispatcher::dispatch_getpgid(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SETPRIORITY.raw() =>
            KernelSyscallDispatcher::dispatch_setpriority(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GETPRIORITY.raw() =>
            KernelSyscallDispatcher::dispatch_getpriority(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::WAITPID.raw() =>
            KernelSyscallDispatcher::dispatch_waitpid(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::WAITID.raw() =>
            KernelSyscallDispatcher::dispatch_waitid(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::KILL.raw() =>
            KernelSyscallDispatcher::dispatch_kill(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::NANOSLEEP.raw() =>
            KernelSyscallDispatcher::dispatch_nanosleep(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::UNAME.raw() =>
            KernelSyscallDispatcher::dispatch_uname(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SYSLOG.raw() =>
            KernelSyscallDispatcher::dispatch_syslog(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::PRCTL.raw() =>
            KernelSyscallDispatcher::dispatch_prctl(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::CAPGET.raw() =>
            KernelSyscallDispatcher::dispatch_capget(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::CAPSET.raw() =>
            KernelSyscallDispatcher::dispatch_capset(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GETCWD.raw() =>
            KernelSyscallDispatcher::dispatch_getcwd(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::CHDIR.raw() =>
            KernelSyscallDispatcher::dispatch_chdir(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::FUTEX.raw() =>
            KernelSyscallDispatcher::dispatch_futex(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::RT_SIGACTION.raw() =>
            KernelSyscallDispatcher::dispatch_rt_sigaction(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::RT_SIGPROCMASK.raw() =>
            KernelSyscallDispatcher::dispatch_rt_sigprocmask(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::RT_SIGPENDING.raw() =>
            KernelSyscallDispatcher::dispatch_rt_sigpending(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::RT_SIGSUSPEND.raw() =>
            KernelSyscallDispatcher::dispatch_rt_sigsuspend(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::RT_SIGTIMEDWAIT.raw() =>
            KernelSyscallDispatcher::dispatch_rt_sigtimedwait(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::RT_SIGRETURN.raw() =>
            KernelSyscallDispatcher::dispatch_rt_sigreturn(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::TKILL.raw() =>
            KernelSyscallDispatcher::dispatch_tkill(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::TGKILL.raw() =>
            KernelSyscallDispatcher::dispatch_tgkill(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SET_TID_ADDRESS.raw() =>
            KernelSyscallDispatcher::dispatch_set_tid_address(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SET_ROBUST_LIST.raw() =>
            KernelSyscallDispatcher::dispatch_set_robust_list(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GET_ROBUST_LIST.raw() =>
            KernelSyscallDispatcher::dispatch_get_robust_list(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GETRANDOM.raw() =>
            KernelSyscallDispatcher::dispatch_getrandom(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GETITIMER.raw() =>
            KernelSyscallDispatcher::dispatch_getitimer(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SETITIMER.raw() =>
            KernelSyscallDispatcher::dispatch_setitimer(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GETRLIMIT.raw() =>
            KernelSyscallDispatcher::dispatch_getrlimit(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GETRUSAGE.raw() =>
            KernelSyscallDispatcher::dispatch_getrusage(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SETRLIMIT.raw() =>
            KernelSyscallDispatcher::dispatch_setrlimit(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::UMASK.raw() =>
            KernelSyscallDispatcher::dispatch_umask(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::PRLIMIT64.raw() =>
            KernelSyscallDispatcher::dispatch_prlimit64(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SOCKET.raw() =>
            KernelSyscallDispatcher::dispatch_socket(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SOCKETPAIR.raw() =>
            KernelSyscallDispatcher::dispatch_socketpair(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::BIND.raw() =>
            KernelSyscallDispatcher::dispatch_bind(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::LISTEN.raw() =>
            KernelSyscallDispatcher::dispatch_listen(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::ACCEPT.raw() =>
            KernelSyscallDispatcher::dispatch_accept(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::ACCEPT4.raw() =>
            KernelSyscallDispatcher::dispatch_accept4(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::CONNECT.raw() =>
            KernelSyscallDispatcher::dispatch_connect(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GETSOCKNAME.raw() =>
            KernelSyscallDispatcher::dispatch_getsockname(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GETPEERNAME.raw() =>
            KernelSyscallDispatcher::dispatch_getpeername(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SENDTO.raw() =>
            KernelSyscallDispatcher::dispatch_sendto(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::RECVFROM.raw() =>
            KernelSyscallDispatcher::dispatch_recvfrom(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SENDMSG.raw() =>
            KernelSyscallDispatcher::dispatch_sendmsg(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::RECVMSG.raw() =>
            KernelSyscallDispatcher::dispatch_recvmsg(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SETSOCKOPT.raw() =>
            KernelSyscallDispatcher::dispatch_setsockopt(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::GETSOCKOPT.raw() =>
            KernelSyscallDispatcher::dispatch_getsockopt(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SHUTDOWN.raw() =>
            KernelSyscallDispatcher::dispatch_shutdown(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::PPOLL.raw() =>
            KernelSyscallDispatcher::dispatch_ppoll(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::PSELECT6.raw() =>
            KernelSyscallDispatcher::dispatch_pselect6(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::POLL.raw() =>
            KernelSyscallDispatcher::dispatch_poll(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::SELECT.raw() =>
            KernelSyscallDispatcher::dispatch_select(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::EPOLL_CREATE1.raw() =>
            KernelSyscallDispatcher::dispatch_epoll_create1(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::EPOLL_CTL.raw() =>
            KernelSyscallDispatcher::dispatch_epoll_ctl(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::EPOLL_WAIT.raw() =>
            KernelSyscallDispatcher::dispatch_epoll_wait(syscall_args),
        n if n == <ActiveSyscallNumberTable as SyscallNumberTable>::EPOLL_PWAIT.raw() =>
            KernelSyscallDispatcher::dispatch_epoll_pwait(syscall_args),
        _ => dispatch_syscall_aliases(syscall_nr, syscall_args),
    }
}

/// EINTR 后可重启 syscall。
#[inline]
// 本方法代码由AI完成
pub fn is_restartable_syscall_nr(syscall_nr: usize) -> bool {
    use ActiveSyscallNumberTable as T;
    syscall_nr == <T as SyscallNumberTable>::READ.raw()
        || syscall_nr == <T as SyscallNumberTable>::READV.raw()
        || syscall_nr == <T as SyscallNumberTable>::WRITE.raw()
        || syscall_nr == <T as SyscallNumberTable>::WRITEV.raw()
        || syscall_nr == <T as SyscallNumberTable>::WAITPID.raw()
        || syscall_nr == <T as SyscallNumberTable>::WAITID.raw()
        || syscall_nr == <T as SyscallNumberTable>::ACCEPT4.raw()
        || syscall_nr == <T as SyscallNumberTable>::CONNECT.raw()
        || syscall_nr == <T as SyscallNumberTable>::SENDTO.raw()
        || syscall_nr == <T as SyscallNumberTable>::RECVFROM.raw()
        || syscall_nr == <T as SyscallNumberTable>::SENDMSG.raw()
        || syscall_nr == <T as SyscallNumberTable>::RECVMSG.raw()
}
