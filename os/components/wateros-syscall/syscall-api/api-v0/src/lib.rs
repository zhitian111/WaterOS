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
    Readv,
    Write,
    Writev,
    Pread64,
    Pwrite64,
    Preadv,
    Pwritev,
    Sendfile,
    ReadLinkAt,
    FaccessAt,
    StatFs,
    Sync,
    Fsync,
    Fdatasync,
    Ftruncate,
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
    RenameAt2,
    UtimensAt,
    Mount,
    Umount2,
    Yield,
    SchedSetparam,
    SchedSetscheduler,
    SchedGetscheduler,
    SchedGetparam,
    SchedSetaffinity,
    SchedGetaffinity,
    Exit,
    ExitGroup,
    Clone,
    Clone3,
    Execve,
    WaitPid,
    Kill,
    Brk,
    Mmap,
    Munmap,
    Msync,
    Mprotect,
    GetMempolicy,
    GetTime,
    ClockSetTime,
    ClockGetTime,
    ClockGetRes,
    ClockNanosleep,
    Nanosleep,
    GetPid,
    GetPPid,
    GetTid,
    GetUid,
    GetEuid,
    GetGid,
    GetEgid,
    SetSid,
    GetGroups,
    Sysinfo,
    SetUid,
    SetGid,
    SetReuid,
    SetRegid,
    SetResuid,
    SetResgid,
    Times,
    Uname,
    Syslog,
    Prctl,
    GetCwd,
    Chdir,
    Futex,
    RtSigaction,
    RtSigprocmask,
    RtSigpending,
    RtSigsuspend,
    RtSigtimedwait,
    RtSigreturn,
    Tkill,
    Tgkill,
    SetTidAddress,
    SetRobustList,
    GetRobustList,
    GetRandom,
    GetItimer,
    SetItimer,
    GetRlimit,
    GetRusage,
    SetRlimit,
    Umask,
    PrLimit64,
    Socket,
    Bind,
    Listen,
    Accept,
    Accept4,
    Connect,
    GetSockName,
    GetPeerName,
    SendTo,
    RecvFrom,
    SendMsg,
    RecvMsg,
    SetSockOpt,
    GetSockOpt,
    Shutdown,
    Ppoll,
    Pselect6,
    Select,
    Poll,
    Unknown(usize),
}

#[cfg(feature = "api-v0")]
impl SyscallKind {
    /// Decode a raw syscall number using the selected ABI number table.
    #[inline]
    pub fn decode<T: SyscallNumberTable>(syscall_nr: usize) -> Self {
        if syscall_nr == T::READ.raw() {
            Self::Read
        } else if syscall_nr == T::READV.raw() {
            Self::Readv
        } else if syscall_nr == T::WRITE.raw() {
            Self::Write
        } else if syscall_nr == T::WRITEV.raw() {
            Self::Writev
        } else if syscall_nr == T::PREAD64.raw() {
            Self::Pread64
        } else if syscall_nr == T::PWRITE64.raw() {
            Self::Pwrite64
        } else if syscall_nr == T::PREADV.raw() {
            Self::Preadv
        } else if syscall_nr == T::PWRITEV.raw() {
            Self::Pwritev
        } else if syscall_nr == T::SENDFILE.raw() {
            Self::Sendfile
        } else if syscall_nr == T::READLINKAT.raw() {
            Self::ReadLinkAt
        } else if syscall_nr == T::FACCESSAT.raw() {
            Self::FaccessAt
        } else if syscall_nr == T::STATFS.raw() {
            Self::StatFs
        } else if syscall_nr == T::SYNC.raw() {
            Self::Sync
        } else if syscall_nr == T::FSYNC.raw() {
            Self::Fsync
        } else if syscall_nr == T::FDATASYNC.raw() {
            Self::Fdatasync
        } else if syscall_nr == T::FTRUNCATE.raw() {
            Self::Ftruncate
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
        } else if syscall_nr == T::RENAMEAT2.raw() {
            Self::RenameAt2
        } else if syscall_nr == T::UTIMENSAT.raw() {
            Self::UtimensAt
        } else if syscall_nr == T::MOUNT.raw() {
            Self::Mount
        } else if syscall_nr == T::UMOUNT2.raw() {
            Self::Umount2
        } else if syscall_nr == T::SCHED_SETPARAM.raw() {
            Self::SchedSetparam
        } else if syscall_nr == T::SCHED_SETSCHEDULER.raw() {
            Self::SchedSetscheduler
        } else if syscall_nr == T::SCHED_GETSCHEDULER.raw() {
            Self::SchedGetscheduler
        } else if syscall_nr == T::SCHED_GETPARAM.raw() {
            Self::SchedGetparam
        } else if syscall_nr == T::SCHED_SETAFFINITY.raw() {
            Self::SchedSetaffinity
        } else if syscall_nr == T::SCHED_GETAFFINITY.raw() {
            Self::SchedGetaffinity
        } else if syscall_nr == T::YIELD.raw() {
            Self::Yield
        } else if syscall_nr == T::EXIT.raw() {
            Self::Exit
        } else if syscall_nr == T::EXIT_GROUP.raw() {
            Self::ExitGroup
        } else if syscall_nr == T::FORK.raw() {
            Self::Clone
        } else if syscall_nr == T::CLONE3.raw() {
            Self::Clone3
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
        } else if syscall_nr == T::MSYNC.raw() {
            Self::Msync
        } else if syscall_nr == T::MPROTECT.raw() {
            Self::Mprotect
        } else if syscall_nr == T::GET_MEMPOLICY.raw() {
            Self::GetMempolicy
        } else if syscall_nr == T::GET_TIME.raw() {
            Self::GetTime
        } else if syscall_nr == T::CLOCK_SETTIME.raw() {
            Self::ClockSetTime
        } else if syscall_nr == T::CLOCK_GETTIME.raw() {
            Self::ClockGetTime
        } else if syscall_nr == T::CLOCK_GETRES.raw() {
            Self::ClockGetRes
        } else if syscall_nr == T::CLOCK_NANOSLEEP.raw() {
            Self::ClockNanosleep
        } else if syscall_nr == T::NANOSLEEP.raw() {
            Self::Nanosleep
        } else if syscall_nr == T::GETPID.raw() {
            Self::GetPid
        } else if syscall_nr == T::GETPPID.raw() {
            Self::GetPPid
        } else if syscall_nr == T::GETTID.raw() {
            Self::GetTid
        } else if syscall_nr == T::GETUID.raw() {
            Self::GetUid
        } else if syscall_nr == T::GETEUID.raw() {
            Self::GetEuid
        } else if syscall_nr == T::GETGID.raw() {
            Self::GetGid
        } else if syscall_nr == T::GETEGID.raw() {
            Self::GetEgid
        } else if syscall_nr == T::SETSID.raw() {
            Self::SetSid
        } else if syscall_nr == T::GETGROUPS.raw() {
            Self::GetGroups
        } else if syscall_nr == T::SYSINFO.raw() {
            Self::Sysinfo
        } else if syscall_nr == T::SETUID.raw() {
            Self::SetUid
        } else if syscall_nr == T::SETGID.raw() {
            Self::SetGid
        } else if syscall_nr == T::SETREUID.raw() {
            Self::SetReuid
        } else if syscall_nr == T::SETREGID.raw() {
            Self::SetRegid
        } else if syscall_nr == T::SETRESUID.raw() {
            Self::SetResuid
        } else if syscall_nr == T::SETRESGID.raw() {
            Self::SetResgid
        } else if syscall_nr == T::TIMES.raw() {
            Self::Times
        } else if syscall_nr == T::UNAME.raw() {
            Self::Uname
        } else if syscall_nr == T::SYSLOG.raw() {
            Self::Syslog
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
        } else if syscall_nr == T::RT_SIGPENDING.raw() {
            Self::RtSigpending
        } else if syscall_nr == T::RT_SIGSUSPEND.raw() {
            Self::RtSigsuspend
        } else if syscall_nr == T::RT_SIGTIMEDWAIT.raw() {
            Self::RtSigtimedwait
        } else if syscall_nr == T::RT_SIGRETURN.raw() {
            Self::RtSigreturn
        } else if syscall_nr == T::TKILL.raw() {
            Self::Tkill
        } else if syscall_nr == T::TGKILL.raw() {
            Self::Tgkill
        } else if syscall_nr == T::SET_TID_ADDRESS.raw() {
            Self::SetTidAddress
        } else if syscall_nr == T::SET_ROBUST_LIST.raw() {
            Self::SetRobustList
        } else if syscall_nr == T::GET_ROBUST_LIST.raw() {
            Self::GetRobustList
        } else if syscall_nr == T::GETRANDOM.raw() {
            Self::GetRandom
        } else if syscall_nr == T::GETITIMER.raw() {
            Self::GetItimer
        } else if syscall_nr == T::SETITIMER.raw() {
            Self::SetItimer
        } else if syscall_nr == T::GETRLIMIT.raw() {
            Self::GetRlimit
        } else if syscall_nr == T::GETRUSAGE.raw() {
            Self::GetRusage
        } else if syscall_nr == T::SETRLIMIT.raw() {
            Self::SetRlimit
        } else if syscall_nr == T::UMASK.raw() {
            Self::Umask
        } else if syscall_nr == T::PRLIMIT64.raw() {
            Self::PrLimit64
        } else if syscall_nr == T::SOCKET.raw() {
            Self::Socket
        } else if syscall_nr == T::BIND.raw() {
            Self::Bind
        } else if syscall_nr == T::LISTEN.raw() {
            Self::Listen
        } else if syscall_nr == T::ACCEPT.raw() {
            Self::Accept
        } else if syscall_nr == T::ACCEPT4.raw() {
            Self::Accept4
        } else if syscall_nr == T::CONNECT.raw() {
            Self::Connect
        } else if syscall_nr == T::GETSOCKNAME.raw() {
            Self::GetSockName
        } else if syscall_nr == T::GETPEERNAME.raw() {
            Self::GetPeerName
        } else if syscall_nr == T::SENDTO.raw() {
            Self::SendTo
        } else if syscall_nr == T::RECVFROM.raw() {
            Self::RecvFrom
        } else if syscall_nr == T::SENDMSG.raw() {
            Self::SendMsg
        } else if syscall_nr == T::RECVMSG.raw() {
            Self::RecvMsg
        } else if syscall_nr == T::SETSOCKOPT.raw() {
            Self::SetSockOpt
        } else if syscall_nr == T::GETSOCKOPT.raw() {
            Self::GetSockOpt
        } else if syscall_nr == T::SHUTDOWN.raw() {
            Self::Shutdown
        } else if syscall_nr == T::PPOLL.raw() {
            Self::Ppoll
        } else if syscall_nr == T::PSELECT6.raw() {
            Self::Pselect6
        } else if syscall_nr == T::SELECT.raw() {
            Self::Select
        } else if syscall_nr == T::POLL.raw() {
            Self::Poll
        } else {
            Self::Unknown(syscall_nr)
        }
    }

    /// Stable syscall slot name for logs and bring-up diagnostics.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Readv => "readv",
            Self::Write => "write",
            Self::Writev => "writev",
            Self::Pread64 => "pread64",
            Self::Pwrite64 => "pwrite64",
            Self::Preadv => "preadv",
            Self::Pwritev => "pwritev",
            Self::Sendfile => "sendfile",
            Self::ReadLinkAt => "readlinkat",
            Self::FaccessAt => "faccessat",
            Self::StatFs => "statfs",
            Self::Sync => "sync",
            Self::Fsync => "fsync",
            Self::Fdatasync => "fdatasync",
            Self::Ftruncate => "ftruncate",
            Self::OpenAt => "openat",
            Self::Close => "close",
            Self::Fstat => "fstat",
            Self::Lseek => "lseek",
            Self::Dup => "dup",
            Self::Dup3 => "dup3",
            Self::Pipe2 => "pipe2",
            Self::Ioctl => "ioctl",
            Self::Fcntl => "fcntl",
            Self::GetDents64 => "getdents64",
            Self::MkdirAt => "mkdirat",
            Self::UnlinkAt => "unlinkat",
            Self::RenameAt2 => "renameat2",
            Self::UtimensAt => "utimensat",
            Self::Mount => "mount",
            Self::Umount2 => "umount2",
            Self::Yield => "sched_yield",
            Self::SchedSetparam => "sched_setparam",
            Self::SchedSetscheduler => "sched_setscheduler",
            Self::SchedGetscheduler => "sched_getscheduler",
            Self::SchedGetparam => "sched_getparam",
            Self::SchedSetaffinity => "sched_setaffinity",
            Self::SchedGetaffinity => "sched_getaffinity",
            Self::Exit => "exit",
            Self::ExitGroup => "exit_group",
            Self::Clone => "clone",
            Self::Clone3 => "clone3",
            Self::Execve => "execve",
            Self::WaitPid => "waitpid",
            Self::Kill => "kill",
            Self::Brk => "brk",
            Self::Mmap => "mmap",
            Self::Munmap => "munmap",
            Self::Msync => "msync",
            Self::Mprotect => "mprotect",
            Self::GetMempolicy => "get_mempolicy",
            Self::GetTime => "gettimeofday",
            Self::ClockSetTime => "clock_settime",
            Self::ClockGetTime => "clock_gettime",
            Self::ClockGetRes => "clock_getres",
            Self::ClockNanosleep => "clock_nanosleep",
            Self::Nanosleep => "nanosleep",
            Self::GetPid => "getpid",
            Self::GetPPid => "getppid",
            Self::GetTid => "gettid",
            Self::GetUid => "getuid",
            Self::GetEuid => "geteuid",
            Self::GetGid => "getgid",
            Self::GetEgid => "getegid",
            Self::SetSid => "setsid",
            Self::GetGroups => "getgroups",
            Self::Sysinfo => "sysinfo",
            Self::SetUid => "setuid",
            Self::SetGid => "setgid",
            Self::SetReuid => "setreuid",
            Self::SetRegid => "setregid",
            Self::SetResuid => "setresuid",
            Self::SetResgid => "setresgid",
            Self::Times => "times",
            Self::Uname => "uname",
            Self::Syslog => "syslog",
            Self::Prctl => "prctl",
            Self::GetCwd => "getcwd",
            Self::Chdir => "chdir",
            Self::Futex => "futex",
            Self::RtSigaction => "rt_sigaction",
            Self::RtSigprocmask => "rt_sigprocmask",
            Self::RtSigpending => "rt_sigpending",
            Self::RtSigsuspend => "rt_sigsuspend",
            Self::RtSigtimedwait => "rt_sigtimedwait",
            Self::RtSigreturn => "rt_sigreturn",
            Self::Tkill => "tkill",
            Self::Tgkill => "tgkill",
            Self::SetTidAddress => "set_tid_address",
            Self::SetRobustList => "set_robust_list",
            Self::GetRobustList => "get_robust_list",
            Self::GetRandom => "getrandom",
            Self::GetItimer => "getitimer",
            Self::SetItimer => "setitimer",
            Self::GetRlimit => "getrlimit",
            Self::GetRusage => "getrusage",
            Self::SetRlimit => "setrlimit",
            Self::Umask => "umask",
            Self::PrLimit64 => "prlimit64",
            Self::Socket => "socket",
            Self::Bind => "bind",
            Self::Listen => "listen",
            Self::Accept => "accept",
            Self::Accept4 => "accept4",
            Self::Connect => "connect",
            Self::GetSockName => "getsockname",
            Self::GetPeerName => "getpeername",
            Self::SendTo => "sendto",
            Self::RecvFrom => "recvfrom",
            Self::SendMsg => "sendmsg",
            Self::RecvMsg => "recvmsg",
            Self::SetSockOpt => "setsockopt",
            Self::GetSockOpt => "getsockopt",
            Self::Shutdown => "shutdown",
            Self::Ppoll => "ppoll",
            Self::Pselect6 => "pselect6",
            Self::Select => "select",
            Self::Poll => "poll",
            Self::Unknown(_) => "unknown",
        }
    }
}

/// Bring-up helpers for decoded-but-unimplemented syscall slots and unknown numbers.
#[cfg(feature = "api-v0")]
pub mod unsupported {
    extern crate alloc;

    use abi::syscall_args::SyscallArgs;

    use super::SyscallKind;

    /// Label for a decoded syscall slot (same as [`SyscallKind::label`]).
    #[inline]
    #[must_use]
    pub const fn slot_label(kind: SyscallKind) -> &'static str {
        kind.label()
    }

    /// 报告不支持的 syscall 语义并终止内核（不返回用户态）。
    #[inline(never)]
    pub fn syscall_unsupported(detail: &str) -> ! {
        panic!("[syscall] unsupported: {detail}");
    }

    /// 已解码槽位尚未实现（见 [`SyscallKind::label`]）。
    #[inline(never)]
    pub fn syscall_unsupported_decoded(kind: SyscallKind, nr: usize, args: SyscallArgs) -> ! {
        syscall_unsupported_slot(kind.label(), nr, args);
    }

    #[inline(never)]
    pub fn syscall_unsupported_slot(label: &str, nr: usize, args: SyscallArgs) -> ! {
        let r = args.as_regs();
        syscall_unsupported(&alloc::format!(
            "{label} nr={nr} args=[{:#x},{:#x},{:#x},{:#x},{:#x},{:#x}]",
            r[0],
            r[1],
            r[2],
            r[3],
            r[4],
            r[5]
        ));
    }

    /// 号表未收录的 syscall 号（[`SyscallKind::Unknown`]）。
    #[inline(never)]
    pub fn syscall_unknown(nr: usize, args: SyscallArgs) -> ! {
        syscall_unsupported_slot("unknown", nr, args);
    }
}

/// Standard return value for decoded but not-yet-implemented syscall slots.
#[cfg(feature = "api-v0")]
#[inline]
pub fn syscall_enosys_ret() -> isize {
    UserRet::from_error(ErrNo::ENOSYS).0
}

/// Kernel-side syscall dispatcher selected by the aggregate crate.
#[cfg(feature = "api-v0")]
pub trait SyscallDispatcher {
    /// ABI number table used to decode raw syscall IDs.
    type NumberTable: SyscallNumberTable;

    fn dispatch_yield(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Yield,
            Self::NumberTable::YIELD.raw(),
            args,
        )
    }

    fn dispatch_sched_setparam(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::SchedSetparam,
            Self::NumberTable::SCHED_SETPARAM.raw(),
            args,
        )
    }

    fn dispatch_sched_setscheduler(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::SchedSetscheduler,
            Self::NumberTable::SCHED_SETSCHEDULER.raw(),
            args,
        )
    }

    fn dispatch_sched_getparam(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::SchedGetparam,
            Self::NumberTable::SCHED_GETPARAM.raw(),
            args,
        )
    }

    fn dispatch_sched_getscheduler(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::SchedGetscheduler,
            Self::NumberTable::SCHED_GETSCHEDULER.raw(),
            args,
        )
    }

    fn dispatch_sched_setaffinity(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::SchedSetaffinity,
            Self::NumberTable::SCHED_SETAFFINITY.raw(),
            args,
        )
    }

    fn dispatch_sched_getaffinity(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::SchedGetaffinity,
            Self::NumberTable::SCHED_GETAFFINITY.raw(),
            args,
        )
    }

    fn dispatch_exit(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Exit,
            Self::NumberTable::EXIT.raw(),
            args,
        )
    }

    fn dispatch_exit_group(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::ExitGroup,
            Self::NumberTable::EXIT_GROUP.raw(),
            args,
        )
    }

    fn dispatch_read(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Read,
            Self::NumberTable::READ.raw(),
            args,
        )
    }

    fn dispatch_readv(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Readv,
            Self::NumberTable::READV.raw(),
            args,
        )
    }

    fn dispatch_write(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Write,
            Self::NumberTable::WRITE.raw(),
            args,
        )
    }

    fn dispatch_writev(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Writev,
            Self::NumberTable::WRITEV.raw(),
            args,
        )
    }

    fn dispatch_pread64(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Pread64,
            Self::NumberTable::PREAD64.raw(),
            args,
        )
    }

    fn dispatch_pwrite64(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Pwrite64,
            Self::NumberTable::PWRITE64.raw(),
            args,
        )
    }

    fn dispatch_preadv(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Preadv,
            Self::NumberTable::PREADV.raw(),
            args,
        )
    }

    fn dispatch_pwritev(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Pwritev,
            Self::NumberTable::PWRITEV.raw(),
            args,
        )
    }

    fn dispatch_sendfile(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Sendfile,
            Self::NumberTable::SENDFILE.raw(),
            args,
        )
    }

    fn dispatch_readlinkat(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::ReadLinkAt,
            Self::NumberTable::READLINKAT.raw(),
            args,
        )
    }

    fn dispatch_faccessat(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::FaccessAt,
            Self::NumberTable::FACCESSAT.raw(),
            args,
        )
    }

    fn dispatch_statfs(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::StatFs,
            Self::NumberTable::STATFS.raw(),
            args,
        )
    }

    fn dispatch_sync(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Sync,
            Self::NumberTable::SYNC.raw(),
            args,
        )
    }

    fn dispatch_fsync(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Fsync,
            Self::NumberTable::FSYNC.raw(),
            args,
        )
    }

    fn dispatch_fdatasync(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Fdatasync,
            Self::NumberTable::FDATASYNC.raw(),
            args,
        )
    }

    fn dispatch_ftruncate(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Ftruncate,
            Self::NumberTable::FTRUNCATE.raw(),
            args,
        )
    }

    fn dispatch_openat(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::OpenAt,
            Self::NumberTable::OPENAT.raw(),
            args,
        )
    }

    fn dispatch_close(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Close,
            Self::NumberTable::CLOSE.raw(),
            args,
        )
    }

    fn dispatch_fstat(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Fstat,
            Self::NumberTable::FSTAT.raw(),
            args,
        )
    }

    fn dispatch_lseek(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Lseek,
            Self::NumberTable::LSEEK.raw(),
            args,
        )
    }

    fn dispatch_dup(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Dup,
            Self::NumberTable::DUP.raw(),
            args,
        )
    }

    fn dispatch_dup3(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Dup3,
            Self::NumberTable::DUP3.raw(),
            args,
        )
    }

    fn dispatch_pipe2(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Pipe2,
            Self::NumberTable::PIPE2.raw(),
            args,
        )
    }

    fn dispatch_ioctl(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Ioctl,
            Self::NumberTable::IOCTL.raw(),
            args,
        )
    }

    fn dispatch_fcntl(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Fcntl,
            Self::NumberTable::FCNTL.raw(),
            args,
        )
    }

    fn dispatch_getdents64(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::GetDents64,
            Self::NumberTable::GETDENTS64.raw(),
            args,
        )
    }

    fn dispatch_mkdirat(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::MkdirAt,
            Self::NumberTable::MKDIRAT.raw(),
            args,
        )
    }

    fn dispatch_unlinkat(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::UnlinkAt,
            Self::NumberTable::UNLINKAT.raw(),
            args,
        )
    }

    fn dispatch_renameat2(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::RenameAt2,
            Self::NumberTable::RENAMEAT2.raw(),
            args,
        )
    }

    fn dispatch_utimensat(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::UtimensAt,
            Self::NumberTable::UTIMENSAT.raw(),
            args,
        )
    }

    fn dispatch_mount(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Mount,
            Self::NumberTable::MOUNT.raw(),
            args,
        )
    }

    fn dispatch_umount2(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Umount2,
            Self::NumberTable::UMOUNT2.raw(),
            args,
        )
    }

    fn dispatch_clone(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Clone,
            Self::NumberTable::FORK.raw(),
            args,
        )
    }

    fn dispatch_clone3(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Clone3,
            Self::NumberTable::CLONE3.raw(),
            args,
        )
    }

    fn dispatch_execve(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Execve,
            Self::NumberTable::EXEC.raw(),
            args,
        )
    }

    fn dispatch_brk(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Brk,
            Self::NumberTable::BRK.raw(),
            args,
        )
    }

    fn dispatch_mmap(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Mmap,
            Self::NumberTable::MMAP.raw(),
            args,
        )
    }

    fn dispatch_munmap(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Munmap,
            Self::NumberTable::MUNMAP.raw(),
            args,
        )
    }

    fn dispatch_msync(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Msync,
            Self::NumberTable::MSYNC.raw(),
            args,
        )
    }

    fn dispatch_mprotect(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Mprotect,
            Self::NumberTable::MPROTECT.raw(),
            args,
        )
    }

    fn dispatch_getmempolicy(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::GetMempolicy,
            Self::NumberTable::GET_MEMPOLICY.raw(),
            args,
        )
    }

    fn dispatch_get_time(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::GetTime,
            Self::NumberTable::GET_TIME.raw(),
            args,
        )
    }

    fn dispatch_clock_gettime(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::ClockGetTime,
            Self::NumberTable::CLOCK_GETTIME.raw(),
            args,
        )
    }

    fn dispatch_clock_settime(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::ClockSetTime,
            Self::NumberTable::CLOCK_SETTIME.raw(),
            args,
        )
    }

    fn dispatch_clock_getres(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::ClockGetRes,
            Self::NumberTable::CLOCK_GETRES.raw(),
            args,
        )
    }

    fn dispatch_clock_nanosleep(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::ClockNanosleep,
            Self::NumberTable::CLOCK_NANOSLEEP.raw(),
            args,
        )
    }

    fn dispatch_getpid(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::GetPid,
            Self::NumberTable::GETPID.raw(),
            args,
        )
    }

    fn dispatch_getppid(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::GetPPid,
            Self::NumberTable::GETPPID.raw(),
            args,
        )
    }

    fn dispatch_gettid(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::GetTid,
            Self::NumberTable::GETTID.raw(),
            args,
        )
    }

    fn dispatch_getuid(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::GetUid,
            Self::NumberTable::GETUID.raw(),
            args,
        )
    }

    fn dispatch_geteuid(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::GetEuid,
            Self::NumberTable::GETEUID.raw(),
            args,
        )
    }

    fn dispatch_getgid(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::GetGid,
            Self::NumberTable::GETGID.raw(),
            args,
        )
    }

    fn dispatch_getegid(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::GetEgid,
            Self::NumberTable::GETEGID.raw(),
            args,
        )
    }

    fn dispatch_setsid(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::SetSid,
            Self::NumberTable::SETSID.raw(),
            args,
        )
    }

    fn dispatch_getgroups(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::GetGroups,
            Self::NumberTable::GETGROUPS.raw(),
            args,
        )
    }

    fn dispatch_sysinfo(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Sysinfo,
            Self::NumberTable::SYSINFO.raw(),
            args,
        )
    }

    fn dispatch_setuid(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::SetUid,
            Self::NumberTable::SETUID.raw(),
            args,
        )
    }

    fn dispatch_setgid(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::SetGid,
            Self::NumberTable::SETGID.raw(),
            args,
        )
    }

    fn dispatch_setreuid(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::SetReuid,
            Self::NumberTable::SETREUID.raw(),
            args,
        )
    }

    fn dispatch_setregid(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::SetRegid,
            Self::NumberTable::SETREGID.raw(),
            args,
        )
    }

    fn dispatch_setresuid(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::SetResuid,
            Self::NumberTable::SETRESUID.raw(),
            args,
        )
    }

    fn dispatch_setresgid(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::SetResgid,
            Self::NumberTable::SETRESGID.raw(),
            args,
        )
    }

    fn dispatch_times(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Times,
            Self::NumberTable::TIMES.raw(),
            args,
        )
    }

    fn dispatch_waitpid(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::WaitPid,
            Self::NumberTable::WAITPID.raw(),
            args,
        )
    }

    fn dispatch_kill(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Kill,
            Self::NumberTable::KILL.raw(),
            args,
        )
    }

    fn dispatch_nanosleep(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Nanosleep,
            Self::NumberTable::NANOSLEEP.raw(),
            args,
        )
    }

    fn dispatch_uname(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Uname,
            Self::NumberTable::UNAME.raw(),
            args,
        )
    }

    fn dispatch_syslog(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Syslog,
            Self::NumberTable::SYSLOG.raw(),
            args,
        )
    }

    fn dispatch_prctl(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Prctl,
            Self::NumberTable::PRCTL.raw(),
            args,
        )
    }

    fn dispatch_getcwd(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::GetCwd,
            Self::NumberTable::GETCWD.raw(),
            args,
        )
    }

    fn dispatch_chdir(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Chdir,
            Self::NumberTable::CHDIR.raw(),
            args,
        )
    }

    fn dispatch_futex(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Futex,
            Self::NumberTable::FUTEX.raw(),
            args,
        )
    }

    fn dispatch_rt_sigaction(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::RtSigaction,
            Self::NumberTable::RT_SIGACTION.raw(),
            args,
        )
    }

    fn dispatch_rt_sigprocmask(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::RtSigprocmask,
            Self::NumberTable::RT_SIGPROCMASK.raw(),
            args,
        )
    }

    fn dispatch_rt_sigpending(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::RtSigpending,
            Self::NumberTable::RT_SIGPENDING.raw(),
            args,
        )
    }

    fn dispatch_rt_sigsuspend(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::RtSigsuspend,
            Self::NumberTable::RT_SIGSUSPEND.raw(),
            args,
        )
    }

    fn dispatch_rt_sigtimedwait(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::RtSigtimedwait,
            Self::NumberTable::RT_SIGTIMEDWAIT.raw(),
            args,
        )
    }

    fn dispatch_rt_sigreturn(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::RtSigreturn,
            Self::NumberTable::RT_SIGRETURN.raw(),
            args,
        )
    }

    fn dispatch_tkill(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(SyscallKind::Tkill, Self::NumberTable::TKILL.raw(), args)
    }

    fn dispatch_tgkill(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(SyscallKind::Tgkill, Self::NumberTable::TGKILL.raw(), args)
    }

    fn dispatch_set_tid_address(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::SetTidAddress,
            Self::NumberTable::SET_TID_ADDRESS.raw(),
            args,
        )
    }

    fn dispatch_set_robust_list(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::SetRobustList,
            Self::NumberTable::SET_ROBUST_LIST.raw(),
            args,
        )
    }

    fn dispatch_get_robust_list(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::GetRobustList,
            Self::NumberTable::GET_ROBUST_LIST.raw(),
            args,
        )
    }

    fn dispatch_getrandom(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::GetRandom,
            Self::NumberTable::GETRANDOM.raw(),
            args,
        )
    }

    fn dispatch_getitimer(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::GetItimer,
            Self::NumberTable::GETITIMER.raw(),
            args,
        )
    }

    fn dispatch_setitimer(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::SetItimer,
            Self::NumberTable::SETITIMER.raw(),
            args,
        )
    }

    fn dispatch_getrlimit(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::GetRlimit,
            Self::NumberTable::GETRLIMIT.raw(),
            args,
        )
    }

    fn dispatch_getrusage(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::GetRusage,
            Self::NumberTable::GETRUSAGE.raw(),
            args,
        )
    }

    fn dispatch_setrlimit(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::SetRlimit,
            Self::NumberTable::SETRLIMIT.raw(),
            args,
        )
    }

    fn dispatch_umask(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Umask,
            Self::NumberTable::UMASK.raw(),
            args,
        )
    }

    fn dispatch_prlimit64(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::PrLimit64,
            Self::NumberTable::PRLIMIT64.raw(),
            args,
        )
    }

    // ——— socket / 网络 ———

    fn dispatch_socket(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Socket,
            Self::NumberTable::SOCKET.raw(),
            args,
        )
    }

    fn dispatch_bind(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Bind,
            Self::NumberTable::BIND.raw(),
            args,
        )
    }

    fn dispatch_listen(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Listen,
            Self::NumberTable::LISTEN.raw(),
            args,
        )
    }

    fn dispatch_accept4(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Accept4,
            Self::NumberTable::ACCEPT4.raw(),
            args,
        )
    }

    fn dispatch_accept(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Accept,
            Self::NumberTable::ACCEPT.raw(),
            args,
        )
    }

    fn dispatch_connect(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Connect,
            Self::NumberTable::CONNECT.raw(),
            args,
        )
    }

    fn dispatch_getsockname(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::GetSockName,
            Self::NumberTable::GETSOCKNAME.raw(),
            args,
        )
    }

    fn dispatch_getpeername(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::GetPeerName,
            Self::NumberTable::GETPEERNAME.raw(),
            args,
        )
    }

    fn dispatch_sendto(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::SendTo,
            Self::NumberTable::SENDTO.raw(),
            args,
        )
    }

    fn dispatch_recvfrom(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::RecvFrom,
            Self::NumberTable::RECVFROM.raw(),
            args,
        )
    }

    fn dispatch_sendmsg(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::SendMsg,
            Self::NumberTable::SENDMSG.raw(),
            args,
        )
    }

    fn dispatch_recvmsg(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::RecvMsg,
            Self::NumberTable::RECVMSG.raw(),
            args,
        )
    }

    fn dispatch_setsockopt(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::SetSockOpt,
            Self::NumberTable::SETSOCKOPT.raw(),
            args,
        )
    }

    fn dispatch_getsockopt(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::GetSockOpt,
            Self::NumberTable::GETSOCKOPT.raw(),
            args,
        )
    }

    fn dispatch_shutdown(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Shutdown,
            Self::NumberTable::SHUTDOWN.raw(),
            args,
        )
    }

    fn dispatch_ppoll(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Ppoll,
            Self::NumberTable::PPOLL.raw(),
            args,
        )
    }

    fn dispatch_pselect6(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Pselect6,
            Self::NumberTable::PSELECT6.raw(),
            args,
        )
    }

    fn dispatch_select(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Select,
            Self::NumberTable::SELECT.raw(),
            args,
        )
    }

    fn dispatch_poll(args: SyscallArgs) -> isize {
        Self::dispatch_unsupported(
            SyscallKind::Poll,
            Self::NumberTable::POLL.raw(),
            args,
        )
    }

    /// Decoded syscall slot with no kernel `dispatch_*` override yet.
    fn dispatch_unsupported(_kind: SyscallKind, _syscall_nr: usize, _args: SyscallArgs) -> isize {
        syscall_enosys_ret()
    }

    /// Raw syscall number not present in the active ABI number table decode map.
    fn dispatch_unknown(_syscall_nr: usize, _args: SyscallArgs) -> isize {
        syscall_enosys_ret()
    }

    /// Dispatch one syscall packet from a trap frame.
    fn dispatch_syscall_from_trap(syscall_nr: usize, syscall_args: SyscallArgs) -> isize {
        match SyscallKind::decode::<Self::NumberTable>(syscall_nr) {
            SyscallKind::Yield => Self::dispatch_yield(syscall_args),
            SyscallKind::SchedSetparam => Self::dispatch_sched_setparam(syscall_args),
            SyscallKind::SchedSetscheduler => Self::dispatch_sched_setscheduler(syscall_args),
            SyscallKind::SchedGetscheduler => Self::dispatch_sched_getscheduler(syscall_args),
            SyscallKind::SchedGetparam => Self::dispatch_sched_getparam(syscall_args),
            SyscallKind::SchedSetaffinity => Self::dispatch_sched_setaffinity(syscall_args),
            SyscallKind::SchedGetaffinity => Self::dispatch_sched_getaffinity(syscall_args),
            SyscallKind::Exit => Self::dispatch_exit(syscall_args),
            SyscallKind::ExitGroup => Self::dispatch_exit_group(syscall_args),
            SyscallKind::Read => Self::dispatch_read(syscall_args),
            SyscallKind::Readv => Self::dispatch_readv(syscall_args),
            SyscallKind::Write => Self::dispatch_write(syscall_args),
            SyscallKind::Writev => Self::dispatch_writev(syscall_args),
            SyscallKind::Pread64 => Self::dispatch_pread64(syscall_args),
            SyscallKind::Pwrite64 => Self::dispatch_pwrite64(syscall_args),
            SyscallKind::Preadv => Self::dispatch_preadv(syscall_args),
            SyscallKind::Pwritev => Self::dispatch_pwritev(syscall_args),
            SyscallKind::Sendfile => Self::dispatch_sendfile(syscall_args),
            SyscallKind::ReadLinkAt => Self::dispatch_readlinkat(syscall_args),
            SyscallKind::FaccessAt => Self::dispatch_faccessat(syscall_args),
            SyscallKind::StatFs => Self::dispatch_statfs(syscall_args),
            SyscallKind::Sync => Self::dispatch_sync(syscall_args),
            SyscallKind::Fsync => Self::dispatch_fsync(syscall_args),
            SyscallKind::Fdatasync => Self::dispatch_fdatasync(syscall_args),
            SyscallKind::Ftruncate => Self::dispatch_ftruncate(syscall_args),
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
            SyscallKind::RenameAt2 => Self::dispatch_renameat2(syscall_args),
            SyscallKind::UtimensAt => Self::dispatch_utimensat(syscall_args),
            SyscallKind::Mount => Self::dispatch_mount(syscall_args),
            SyscallKind::Umount2 => Self::dispatch_umount2(syscall_args),
            SyscallKind::Brk => Self::dispatch_brk(syscall_args),
            SyscallKind::Clone => Self::dispatch_clone(syscall_args),
            SyscallKind::Clone3 => Self::dispatch_clone3(syscall_args),
            SyscallKind::Execve => Self::dispatch_execve(syscall_args),
            SyscallKind::Mmap => Self::dispatch_mmap(syscall_args),
            SyscallKind::Munmap => Self::dispatch_munmap(syscall_args),
            SyscallKind::Msync => Self::dispatch_msync(syscall_args),
            SyscallKind::Mprotect => Self::dispatch_mprotect(syscall_args),
            SyscallKind::GetMempolicy => Self::dispatch_getmempolicy(syscall_args),
            SyscallKind::GetTime => Self::dispatch_get_time(syscall_args),
            SyscallKind::ClockSetTime => Self::dispatch_clock_settime(syscall_args),
            SyscallKind::ClockGetTime => Self::dispatch_clock_gettime(syscall_args),
            SyscallKind::ClockGetRes => Self::dispatch_clock_getres(syscall_args),
            SyscallKind::ClockNanosleep => Self::dispatch_clock_nanosleep(syscall_args),
            SyscallKind::GetPid => Self::dispatch_getpid(syscall_args),
            SyscallKind::GetPPid => Self::dispatch_getppid(syscall_args),
            SyscallKind::GetTid => Self::dispatch_gettid(syscall_args),
            SyscallKind::GetUid => Self::dispatch_getuid(syscall_args),
            SyscallKind::GetEuid => Self::dispatch_geteuid(syscall_args),
            SyscallKind::GetGid => Self::dispatch_getgid(syscall_args),
            SyscallKind::GetEgid => Self::dispatch_getegid(syscall_args),
            SyscallKind::SetSid => Self::dispatch_setsid(syscall_args),
            SyscallKind::GetGroups => Self::dispatch_getgroups(syscall_args),
            SyscallKind::Sysinfo => Self::dispatch_sysinfo(syscall_args),
            SyscallKind::SetUid => Self::dispatch_setuid(syscall_args),
            SyscallKind::SetGid => Self::dispatch_setgid(syscall_args),
            SyscallKind::SetReuid => Self::dispatch_setreuid(syscall_args),
            SyscallKind::SetRegid => Self::dispatch_setregid(syscall_args),
            SyscallKind::SetResuid => Self::dispatch_setresuid(syscall_args),
            SyscallKind::SetResgid => Self::dispatch_setresgid(syscall_args),
            SyscallKind::Times => Self::dispatch_times(syscall_args),
            SyscallKind::WaitPid => Self::dispatch_waitpid(syscall_args),
            SyscallKind::Kill => Self::dispatch_kill(syscall_args),
            SyscallKind::Nanosleep => Self::dispatch_nanosleep(syscall_args),
            SyscallKind::Uname => Self::dispatch_uname(syscall_args),
            SyscallKind::Syslog => Self::dispatch_syslog(syscall_args),
            SyscallKind::Prctl => Self::dispatch_prctl(syscall_args),
            SyscallKind::GetCwd => Self::dispatch_getcwd(syscall_args),
            SyscallKind::Chdir => Self::dispatch_chdir(syscall_args),
            SyscallKind::Futex => Self::dispatch_futex(syscall_args),
            SyscallKind::RtSigaction => Self::dispatch_rt_sigaction(syscall_args),
            SyscallKind::RtSigprocmask => Self::dispatch_rt_sigprocmask(syscall_args),
            SyscallKind::RtSigpending => Self::dispatch_rt_sigpending(syscall_args),
            SyscallKind::RtSigsuspend => Self::dispatch_rt_sigsuspend(syscall_args),
            SyscallKind::RtSigtimedwait => Self::dispatch_rt_sigtimedwait(syscall_args),
            SyscallKind::RtSigreturn => Self::dispatch_rt_sigreturn(syscall_args),
            SyscallKind::Tkill => Self::dispatch_tkill(syscall_args),
            SyscallKind::Tgkill => Self::dispatch_tgkill(syscall_args),
            SyscallKind::SetTidAddress => Self::dispatch_set_tid_address(syscall_args),
            SyscallKind::SetRobustList => Self::dispatch_set_robust_list(syscall_args),
            SyscallKind::GetRobustList => Self::dispatch_get_robust_list(syscall_args),
            SyscallKind::GetRandom => Self::dispatch_getrandom(syscall_args),
            SyscallKind::GetItimer => Self::dispatch_getitimer(syscall_args),
            SyscallKind::SetItimer => Self::dispatch_setitimer(syscall_args),
            SyscallKind::GetRlimit => Self::dispatch_getrlimit(syscall_args),
            SyscallKind::GetRusage => Self::dispatch_getrusage(syscall_args),
            SyscallKind::SetRlimit => Self::dispatch_setrlimit(syscall_args),
            SyscallKind::Umask => Self::dispatch_umask(syscall_args),
            SyscallKind::PrLimit64 => Self::dispatch_prlimit64(syscall_args),
            SyscallKind::Socket => Self::dispatch_socket(syscall_args),
            SyscallKind::Bind => Self::dispatch_bind(syscall_args),
            SyscallKind::Listen => Self::dispatch_listen(syscall_args),
            SyscallKind::Accept => Self::dispatch_accept(syscall_args),
            SyscallKind::Accept4 => Self::dispatch_accept4(syscall_args),
            SyscallKind::Connect => Self::dispatch_connect(syscall_args),
            SyscallKind::GetSockName => Self::dispatch_getsockname(syscall_args),
            SyscallKind::GetPeerName => Self::dispatch_getpeername(syscall_args),
            SyscallKind::SendTo => Self::dispatch_sendto(syscall_args),
            SyscallKind::RecvFrom => Self::dispatch_recvfrom(syscall_args),
            SyscallKind::SendMsg => Self::dispatch_sendmsg(syscall_args),
            SyscallKind::RecvMsg => Self::dispatch_recvmsg(syscall_args),
            SyscallKind::SetSockOpt => Self::dispatch_setsockopt(syscall_args),
            SyscallKind::GetSockOpt => Self::dispatch_getsockopt(syscall_args),
            SyscallKind::Shutdown => Self::dispatch_shutdown(syscall_args),
            SyscallKind::Ppoll => Self::dispatch_ppoll(syscall_args),
            SyscallKind::Pselect6 => Self::dispatch_pselect6(syscall_args),
            SyscallKind::Select => Self::dispatch_select(syscall_args),
            SyscallKind::Poll => Self::dispatch_poll(syscall_args),
            SyscallKind::Unknown(nr) => Self::dispatch_unknown(nr, syscall_args),
        }
    }
}
