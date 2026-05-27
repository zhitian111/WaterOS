#![no_std]
//! Syscall v0 dispatch contract.
//!
//! This API crate keeps the trap-facing dispatcher shape stable while concrete
//! kernel handlers live in `syscall-impl/*`.

#[cfg(feature = "api-v0")]
use abi::{
    errno::ErrNo, syscall_args::SyscallArgs, syscall_number::SyscallNumberTable, user_ret::UserRet,
};

/// Decoded syscall identity independent of the concrete ABI number table.
#[cfg(feature = "api-v0")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyscallKind {
    Read,
    Write,
    OpenAt,
    Close,
    Fstat,
    Lseek,
    Dup,
    Dup3,
    Pipe2,
    Ioctl,
    Fcntl,
    GetDents64,
    MkdirAt,
    UnlinkAt,
    Mount,
    Umount2,
    Yield,
    Exit,
    Clone,
    Execve,
    WaitPid,
    Kill,
    Brk,
    Mmap,
    Munmap,
    Mprotect,
    GetTime,
    ClockGetTime,
    Nanosleep,
    GetPid,
    GetPPid,
    GetTid,
    Times,
    Uname,
    Prctl,
    GetCwd,
    Chdir,
    Futex,
    RtSigaction,
    RtSigprocmask,
    RtSigreturn,
    SetTidAddress,
    SetRobustList,
    GetRandom,
    SetItimer,
    GetRlimit,
    SetRlimit,
    Unknown(usize),
}

#[cfg(feature = "api-v0")]
impl SyscallKind {
    /// Decode a raw syscall number using the selected ABI number table.
    #[inline]
    pub fn decode<T : SyscallNumberTable>(syscall_nr : usize) -> Self {
        if syscall_nr == T::READ.raw() {
            Self::Read
        } else if syscall_nr == T::WRITE.raw() {
            Self::Write
        } else if syscall_nr == T::OPENAT.raw() {
            Self::OpenAt
        } else if syscall_nr == T::CLOSE.raw() {
            Self::Close
        } else if syscall_nr == T::FSTAT.raw() {
            Self::Fstat
        } else if syscall_nr == T::LSEEK.raw() {
            Self::Lseek
        } else if syscall_nr == T::DUP.raw() {
            Self::Dup
        } else if syscall_nr == T::DUP3.raw() {
            Self::Dup3
        } else if syscall_nr == T::PIPE2.raw() {
            Self::Pipe2
        } else if syscall_nr == T::IOCTL.raw() {
            Self::Ioctl
        } else if syscall_nr == T::FCNTL.raw() {
            Self::Fcntl
        } else if syscall_nr == T::GETDENTS64.raw() {
            Self::GetDents64
        } else if syscall_nr == T::MKDIRAT.raw() {
            Self::MkdirAt
        } else if syscall_nr == T::UNLINKAT.raw() {
            Self::UnlinkAt
        } else if syscall_nr == T::MOUNT.raw() {
            Self::Mount
        } else if syscall_nr == T::UMOUNT2.raw() {
            Self::Umount2
        } else if syscall_nr == T::YIELD.raw() {
            Self::Yield
        } else if syscall_nr == T::EXIT.raw() || syscall_nr == T::EXIT_GROUP.raw() {
            Self::Exit
        } else if syscall_nr == T::FORK.raw() {
            Self::Clone
        } else if syscall_nr == T::EXEC.raw() {
            Self::Execve
        } else if syscall_nr == T::WAITPID.raw() {
            Self::WaitPid
        } else if syscall_nr == T::KILL.raw() {
            Self::Kill
        } else if syscall_nr == T::BRK.raw() {
            Self::Brk
        } else if syscall_nr == T::MMAP.raw() {
            Self::Mmap
        } else if syscall_nr == T::MUNMAP.raw() {
            Self::Munmap
        } else if syscall_nr == T::MPROTECT.raw() {
            Self::Mprotect
        } else if syscall_nr == T::GET_TIME.raw() {
            Self::GetTime
        } else if syscall_nr == T::CLOCK_GETTIME.raw() {
            Self::ClockGetTime
        } else if syscall_nr == T::NANOSLEEP.raw() {
            Self::Nanosleep
        } else if syscall_nr == T::GETPID.raw() {
            Self::GetPid
        } else if syscall_nr == T::GETPPID.raw() {
            Self::GetPPid
        } else if syscall_nr == T::GETTID.raw() {
            Self::GetTid
        } else if syscall_nr == T::TIMES.raw() {
            Self::Times
        } else if syscall_nr == T::UNAME.raw() {
            Self::Uname
        } else if syscall_nr == T::PRCTL.raw() {
            Self::Prctl
        } else if syscall_nr == T::GETCWD.raw() {
            Self::GetCwd
        } else if syscall_nr == T::CHDIR.raw() {
            Self::Chdir
        } else if syscall_nr == T::FUTEX.raw() {
            Self::Futex
        } else if syscall_nr == T::RT_SIGACTION.raw() {
            Self::RtSigaction
        } else if syscall_nr == T::RT_SIGPROCMASK.raw() {
            Self::RtSigprocmask
        } else if syscall_nr == T::RT_SIGRETURN.raw() {
            Self::RtSigreturn
        } else if syscall_nr == T::SET_TID_ADDRESS.raw() {
            Self::SetTidAddress
        } else if syscall_nr == T::SET_ROBUST_LIST.raw() {
            Self::SetRobustList
        } else if syscall_nr == T::GETRANDOM.raw() {
            Self::GetRandom
        } else if syscall_nr == T::SETITIMER.raw() {
            Self::SetItimer
        } else if syscall_nr == T::GETRLIMIT.raw() {
            Self::GetRlimit
        } else if syscall_nr == T::SETRLIMIT.raw() {
            Self::SetRlimit
        } else {
            Self::Unknown(syscall_nr)
        }
    }
}

/// Standard return value for decoded but not-yet-implemented syscall slots.
#[cfg(feature = "api-v0")]
#[inline]
pub fn syscall_enosys_ret() -> isize { UserRet::from_error(ErrNo::ENOSYS).0 }

/// Kernel-side syscall dispatcher selected by the aggregate crate.
#[cfg(feature = "api-v0")]
pub trait SyscallDispatcher {
    /// ABI number table used to decode raw syscall IDs.
    type NumberTable: SyscallNumberTable;

    fn dispatch_yield(args : SyscallArgs) -> isize;

    fn dispatch_exit(args : SyscallArgs) -> isize;

    fn dispatch_read(args : SyscallArgs) -> isize;

    fn dispatch_write(args : SyscallArgs) -> isize;

    fn dispatch_openat(args : SyscallArgs) -> isize;

    fn dispatch_close(args : SyscallArgs) -> isize;

    fn dispatch_fstat(args : SyscallArgs) -> isize;

    fn dispatch_lseek(args : SyscallArgs) -> isize;

    fn dispatch_dup(_args : SyscallArgs) -> isize { syscall_enosys_ret() }

    fn dispatch_dup3(_args : SyscallArgs) -> isize { syscall_enosys_ret() }

    fn dispatch_pipe2(args : SyscallArgs) -> isize;

    fn dispatch_ioctl(_args : SyscallArgs) -> isize { syscall_enosys_ret() }

    fn dispatch_fcntl(_args : SyscallArgs) -> isize { syscall_enosys_ret() }

    fn dispatch_getdents64(_args : SyscallArgs) -> isize { syscall_enosys_ret() }

    fn dispatch_mkdirat(_args : SyscallArgs) -> isize { syscall_enosys_ret() }

    fn dispatch_unlinkat(_args : SyscallArgs) -> isize { syscall_enosys_ret() }

    fn dispatch_mount(_args : SyscallArgs) -> isize { syscall_enosys_ret() }

    fn dispatch_umount2(_args : SyscallArgs) -> isize { syscall_enosys_ret() }

    fn dispatch_clone(_args : SyscallArgs) -> isize { syscall_enosys_ret() }

    fn dispatch_execve(_args : SyscallArgs) -> isize { syscall_enosys_ret() }

    fn dispatch_brk(args : SyscallArgs) -> isize;

    fn dispatch_mmap(args : SyscallArgs) -> isize;

    fn dispatch_munmap(args : SyscallArgs) -> isize;

    fn dispatch_mprotect(args : SyscallArgs) -> isize;

    fn dispatch_get_time(args : SyscallArgs) -> isize;

    fn dispatch_clock_gettime(args : SyscallArgs) -> isize;

    fn dispatch_getpid(args : SyscallArgs) -> isize;

    fn dispatch_getppid(args : SyscallArgs) -> isize;

    fn dispatch_gettid(args : SyscallArgs) -> isize { Self::dispatch_getpid(args) }

    fn dispatch_times(args : SyscallArgs) -> isize;

    fn dispatch_waitpid(args : SyscallArgs) -> isize;

    fn dispatch_kill(_args : SyscallArgs) -> isize { syscall_enosys_ret() }

    fn dispatch_nanosleep(args : SyscallArgs) -> isize;

    fn dispatch_uname(_args : SyscallArgs) -> isize { syscall_enosys_ret() }

    fn dispatch_prctl(_args : SyscallArgs) -> isize { syscall_enosys_ret() }

    fn dispatch_getcwd(args : SyscallArgs) -> isize;

    fn dispatch_chdir(args : SyscallArgs) -> isize;

    fn dispatch_futex(_args : SyscallArgs) -> isize { syscall_enosys_ret() }

    fn dispatch_rt_sigaction(_args : SyscallArgs) -> isize { syscall_enosys_ret() }

    fn dispatch_rt_sigprocmask(_args : SyscallArgs) -> isize { syscall_enosys_ret() }

    fn dispatch_rt_sigreturn(_args : SyscallArgs) -> isize { syscall_enosys_ret() }

    fn dispatch_set_tid_address(args : SyscallArgs) -> isize;

    fn dispatch_set_robust_list(_args : SyscallArgs) -> isize { syscall_enosys_ret() }

    fn dispatch_getrandom(_args : SyscallArgs) -> isize { syscall_enosys_ret() }

    fn dispatch_setitimer(_args : SyscallArgs) -> isize { syscall_enosys_ret() }

    fn dispatch_getrlimit(_args : SyscallArgs) -> isize { syscall_enosys_ret() }

    fn dispatch_setrlimit(_args : SyscallArgs) -> isize { syscall_enosys_ret() }

    fn dispatch_unknown(_syscall_nr : usize, _args : SyscallArgs) -> isize {
        syscall_enosys_ret()
    }

    /// Dispatch one syscall packet from a trap frame.
    fn dispatch_syscall_from_trap(syscall_nr : usize, syscall_args : SyscallArgs) -> isize {
        match SyscallKind::decode::<Self::NumberTable>(syscall_nr) {
            SyscallKind::Yield => Self::dispatch_yield(syscall_args),
            SyscallKind::Exit => Self::dispatch_exit(syscall_args),
            SyscallKind::Read => Self::dispatch_read(syscall_args),
            SyscallKind::Write => Self::dispatch_write(syscall_args),
            SyscallKind::OpenAt => Self::dispatch_openat(syscall_args),
            SyscallKind::Close => Self::dispatch_close(syscall_args),
            SyscallKind::Fstat => Self::dispatch_fstat(syscall_args),
            SyscallKind::Lseek => Self::dispatch_lseek(syscall_args),
            SyscallKind::Dup => Self::dispatch_dup(syscall_args),
            SyscallKind::Dup3 => Self::dispatch_dup3(syscall_args),
            SyscallKind::Pipe2 => Self::dispatch_pipe2(syscall_args),
            SyscallKind::Ioctl => Self::dispatch_ioctl(syscall_args),
            SyscallKind::Fcntl => Self::dispatch_fcntl(syscall_args),
            SyscallKind::GetDents64 => Self::dispatch_getdents64(syscall_args),
            SyscallKind::MkdirAt => Self::dispatch_mkdirat(syscall_args),
            SyscallKind::UnlinkAt => Self::dispatch_unlinkat(syscall_args),
            SyscallKind::Mount => Self::dispatch_mount(syscall_args),
            SyscallKind::Umount2 => Self::dispatch_umount2(syscall_args),
            SyscallKind::Brk => Self::dispatch_brk(syscall_args),
            SyscallKind::Clone => Self::dispatch_clone(syscall_args),
            SyscallKind::Execve => Self::dispatch_execve(syscall_args),
            SyscallKind::Mmap => Self::dispatch_mmap(syscall_args),
            SyscallKind::Munmap => Self::dispatch_munmap(syscall_args),
            SyscallKind::Mprotect => Self::dispatch_mprotect(syscall_args),
            SyscallKind::GetTime => Self::dispatch_get_time(syscall_args),
            SyscallKind::ClockGetTime => Self::dispatch_clock_gettime(syscall_args),
            SyscallKind::GetPid => Self::dispatch_getpid(syscall_args),
            SyscallKind::GetPPid => Self::dispatch_getppid(syscall_args),
            SyscallKind::GetTid => Self::dispatch_gettid(syscall_args),
            SyscallKind::Times => Self::dispatch_times(syscall_args),
            SyscallKind::WaitPid => Self::dispatch_waitpid(syscall_args),
            SyscallKind::Kill => Self::dispatch_kill(syscall_args),
            SyscallKind::Nanosleep => Self::dispatch_nanosleep(syscall_args),
            SyscallKind::Uname => Self::dispatch_uname(syscall_args),
            SyscallKind::Prctl => Self::dispatch_prctl(syscall_args),
            SyscallKind::GetCwd => Self::dispatch_getcwd(syscall_args),
            SyscallKind::Chdir => Self::dispatch_chdir(syscall_args),
            SyscallKind::Futex => Self::dispatch_futex(syscall_args),
            SyscallKind::RtSigaction => Self::dispatch_rt_sigaction(syscall_args),
            SyscallKind::RtSigprocmask => Self::dispatch_rt_sigprocmask(syscall_args),
            SyscallKind::RtSigreturn => Self::dispatch_rt_sigreturn(syscall_args),
            SyscallKind::SetTidAddress => Self::dispatch_set_tid_address(syscall_args),
            SyscallKind::SetRobustList => Self::dispatch_set_robust_list(syscall_args),
            SyscallKind::GetRandom => Self::dispatch_getrandom(syscall_args),
            SyscallKind::SetItimer => Self::dispatch_setitimer(syscall_args),
            SyscallKind::GetRlimit => Self::dispatch_getrlimit(syscall_args),
            SyscallKind::SetRlimit => Self::dispatch_setrlimit(syscall_args),
            SyscallKind::Unknown(nr) => Self::dispatch_unknown(nr, syscall_args),
        }
    }
}
