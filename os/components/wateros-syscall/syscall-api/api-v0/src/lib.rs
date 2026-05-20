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
    Yield,
    Exit,
    Read,
    Write,
    OpenAt,
    Close,
    Fstat,
    Lseek,
    Pipe2,
    Brk,
    Mmap,
    Munmap,
    Mprotect,
    GetTime,
    ClockGetTime,
    GetPid,
    GetPPid,
    GetTid,
    Times,
    WaitPid,
    Nanosleep,
    GetCwd,
    Chdir,
    Unknown(usize),
}

#[cfg(feature = "api-v0")]
impl SyscallKind {
    /// Decode a raw syscall number using the selected ABI number table.
    #[inline]
    pub fn decode<T : SyscallNumberTable>(syscall_nr : usize) -> Self {
        if syscall_nr == T::YIELD.raw() {
            Self::Yield
        } else if syscall_nr == T::EXIT.raw() || syscall_nr == T::EXIT_GROUP.raw() {
            Self::Exit
        } else if syscall_nr == T::READ.raw() {
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
        } else if syscall_nr == T::PIPE2.raw() {
            Self::Pipe2
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
        } else if syscall_nr == T::GETPID.raw() {
            Self::GetPid
        } else if syscall_nr == T::GETPPID.raw() {
            Self::GetPPid
        } else if syscall_nr == T::GETTID.raw() {
            Self::GetTid
        } else if syscall_nr == T::TIMES.raw() {
            Self::Times
        } else if syscall_nr == T::WAITPID.raw() {
            Self::WaitPid
        } else if syscall_nr == T::NANOSLEEP.raw() {
            Self::Nanosleep
        } else if syscall_nr == T::GETCWD.raw() {
            Self::GetCwd
        } else if syscall_nr == T::CHDIR.raw() {
            Self::Chdir
        } else {
            Self::Unknown(syscall_nr)
        }
    }
}

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

    fn dispatch_pipe2(args : SyscallArgs) -> isize;

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

    fn dispatch_nanosleep(args : SyscallArgs) -> isize;

    fn dispatch_getcwd(args : SyscallArgs) -> isize;

    fn dispatch_chdir(args : SyscallArgs) -> isize;

    fn dispatch_unknown(_syscall_nr : usize, _args : SyscallArgs) -> isize {
        UserRet::from_error(ErrNo::ENOSYS).0
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
            SyscallKind::Pipe2 => Self::dispatch_pipe2(syscall_args),
            SyscallKind::Brk => Self::dispatch_brk(syscall_args),
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
            SyscallKind::Nanosleep => Self::dispatch_nanosleep(syscall_args),
            SyscallKind::GetCwd => Self::dispatch_getcwd(syscall_args),
            SyscallKind::Chdir => Self::dispatch_chdir(syscall_args),
            SyscallKind::Unknown(nr) => Self::dispatch_unknown(nr, syscall_args),
        }
    }
}
