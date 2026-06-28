#![no_std]
//! Linux `asm-generic` 风格 64 位系统调用号具体表（`SyscallNumberTable` 实现）。
//!
//! 数值来自 Linux 用户态可见的通用 64 位 ABI；WaterOS 早期用同一张表覆盖
//! RISC-V 64 与 LoongArch64，若未来架构分叉再拆专用 impl。
//!
//! English: concrete syscall number constants for the Linux generic 64-bit ABI;
//! shared across targets until per-arch tables diverge.

use api_v0::syscall_number::{SyscallNumber, SyscallNumberTable};

/// Linux asm-generic 64-bit syscall 号表（早期阶段维护 busybox /
/// 简单进程子集）。
///
/// RISC-V 64 与 LoongArch64 当前都通过 `wateros-abi` 的架构 feature
/// 选择这张表； 后续若架构产生差异，再拆分专用 impl。
///
/// English: zero-sized type whose associated constants are the numeric syscall IDs.
pub struct LinuxGeneric64;

impl SyscallNumberTable for LinuxGeneric64 {
    // I/O / 文件与描述符
    const READ: SyscallNumber = SyscallNumber(63);
    const READV: SyscallNumber = SyscallNumber(65);
    const WRITE: SyscallNumber = SyscallNumber(64);
    const WRITEV: SyscallNumber = SyscallNumber(66);
    const PREAD64: SyscallNumber = SyscallNumber(67);
    const PWRITE64: SyscallNumber = SyscallNumber(68);
    const PREADV: SyscallNumber = SyscallNumber(69);
    const PWRITEV: SyscallNumber = SyscallNumber(70);
    const SENDFILE: SyscallNumber = SyscallNumber(71);
    const PSELECT6: SyscallNumber = SyscallNumber(72);
    const PPOLL: SyscallNumber = SyscallNumber(73);
    /// asm-generic 64 位无独立 `select` nr；riscv64/loong64 用户态经 `pselect6`(72) 实现。
    const SELECT: SyscallNumber = SyscallNumber(usize::MAX);
    const READLINKAT: SyscallNumber = SyscallNumber(78);
    const FACCESSAT: SyscallNumber = SyscallNumber(48);
    const FCHMODAT: SyscallNumber = SyscallNumber(53);
    const FCHOWNAT: SyscallNumber = SyscallNumber(54);
    const STATFS: SyscallNumber = SyscallNumber(43);
    const SYNC: SyscallNumber = SyscallNumber(81);
    const FSYNC: SyscallNumber = SyscallNumber(82);
    const FDATASYNC: SyscallNumber = SyscallNumber(83);
    const FTRUNCATE: SyscallNumber = SyscallNumber(46);
    const FALLOCATE: SyscallNumber = SyscallNumber(47);
    const OPENAT: SyscallNumber = SyscallNumber(56);
    const SETXATTR: SyscallNumber = SyscallNumber(5);
    const LSETXATTR: SyscallNumber = SyscallNumber(6);
    const FSETXATTR: SyscallNumber = SyscallNumber(7);
    const GETXATTR: SyscallNumber = SyscallNumber(8);
    const LGETXATTR: SyscallNumber = SyscallNumber(9);
    const FGETXATTR: SyscallNumber = SyscallNumber(10);
    const LISTXATTR: SyscallNumber = SyscallNumber(11);
    const LLISTXATTR: SyscallNumber = SyscallNumber(12);
    const FLISTXATTR: SyscallNumber = SyscallNumber(13);
    const REMOVEXATTR: SyscallNumber = SyscallNumber(14);
    const LREMOVEXATTR: SyscallNumber = SyscallNumber(15);
    const FREMOVEXATTR: SyscallNumber = SyscallNumber(16);
    const CLOSE: SyscallNumber = SyscallNumber(57);
    const FSTAT: SyscallNumber = SyscallNumber(80);
    const LSEEK: SyscallNumber = SyscallNumber(62);
    const DUP: SyscallNumber = SyscallNumber(23);
    const DUP3: SyscallNumber = SyscallNumber(24);
    const PIPE2: SyscallNumber = SyscallNumber(59);
    const IOCTL: SyscallNumber = SyscallNumber(29);
    const FCNTL: SyscallNumber = SyscallNumber(25);
    const GETDENTS64: SyscallNumber = SyscallNumber(61);
    const MKDIRAT: SyscallNumber = SyscallNumber(34);
    const SYMLINKAT: SyscallNumber = SyscallNumber(36);
    const UNLINKAT: SyscallNumber = SyscallNumber(35);
    const RENAMEAT2: SyscallNumber = SyscallNumber(276);
    const UTIMENSAT: SyscallNumber = SyscallNumber(88);
    const MOUNT: SyscallNumber = SyscallNumber(40);
    const UMOUNT2: SyscallNumber = SyscallNumber(39);

    // 进程/执行 / process & execution
    const EXIT: SyscallNumber = SyscallNumber(93);
    const EXIT_GROUP: SyscallNumber = SyscallNumber(94);
    const FORK: SyscallNumber = SyscallNumber(220); // 常见用户态 fork -> clone
    const CLONE3: SyscallNumber = SyscallNumber(435);
    const WAITPID: SyscallNumber = SyscallNumber(260); // wait4
    const KILL: SyscallNumber = SyscallNumber(129);
    const EXEC: SyscallNumber = SyscallNumber(221); // execve

    // 调度/时间 / scheduling & time
    const SCHED_SETPARAM: SyscallNumber = SyscallNumber(118); // sched_setparam
    const SCHED_SETSCHEDULER: SyscallNumber = SyscallNumber(119); // sched_setscheduler
    const SCHED_GETSCHEDULER: SyscallNumber = SyscallNumber(120); // sched_getscheduler
    const SCHED_GETPARAM: SyscallNumber = SyscallNumber(121); // sched_getparam
    const SCHED_SETAFFINITY: SyscallNumber = SyscallNumber(122); // sched_setaffinity
    const SCHED_GETAFFINITY: SyscallNumber = SyscallNumber(123); // sched_getaffinity
    const SCHED_GET_PRIORITY_MAX: SyscallNumber = SyscallNumber(125); // sched_get_priority_max
    const SCHED_GET_PRIORITY_MIN: SyscallNumber = SyscallNumber(126); // sched_get_priority_min
    const YIELD: SyscallNumber = SyscallNumber(124); // sched_yield
    const GET_TIME: SyscallNumber = SyscallNumber(169); // gettimeofday
    const CLOCK_SETTIME: SyscallNumber = SyscallNumber(112); // clock_settime
    const CLOCK_GETTIME: SyscallNumber = SyscallNumber(113); // clock_gettime
    const CLOCK_GETRES: SyscallNumber = SyscallNumber(114); // clock_getres
    const CLOCK_NANOSLEEP: SyscallNumber = SyscallNumber(115); // clock_nanosleep

    // 内存管理 / memory management
    const BRK: SyscallNumber = SyscallNumber(214); // brk
    const MMAP: SyscallNumber = SyscallNumber(222); // mmap
    const MUNMAP: SyscallNumber = SyscallNumber(215); // munmap
    const MSYNC: SyscallNumber = SyscallNumber(227); // msync
    const MPROTECT: SyscallNumber = SyscallNumber(226); // mprotect
    const MREMAP: SyscallNumber = SyscallNumber(216); // mremap
    const MADVISE: SyscallNumber = SyscallNumber(233); // madvise
    const MLOCK: SyscallNumber = SyscallNumber(228); // mlock
    const MUNLOCK: SyscallNumber = SyscallNumber(229); // munlock
    const MLOCKALL: SyscallNumber = SyscallNumber(230); // mlockall
    const MUNLOCKALL: SyscallNumber = SyscallNumber(231); // munlockall
    const GET_MEMPOLICY: SyscallNumber = SyscallNumber(236); // get_mempolicy
    const SHMGET: SyscallNumber = SyscallNumber(194);
    const SHMCTL: SyscallNumber = SyscallNumber(195);
    const SHMAT: SyscallNumber = SyscallNumber(196);
    const SHMDT: SyscallNumber = SyscallNumber(197);

    // 基本信息 / identity & misc info
    const UNAME: SyscallNumber = SyscallNumber(160); // uname
    const PRCTL: SyscallNumber = SyscallNumber(167); // prctl
    const CAPGET: SyscallNumber = SyscallNumber(90);
    const CAPSET: SyscallNumber = SyscallNumber(91);
    const GETPID: SyscallNumber = SyscallNumber(172);
    const GETPPID: SyscallNumber = SyscallNumber(173);
    const GETCWD: SyscallNumber = SyscallNumber(17);
    const CHDIR: SyscallNumber = SyscallNumber(49);
    const GETTID: SyscallNumber = SyscallNumber(178);
    const TIMES: SyscallNumber = SyscallNumber(153);
    const SETPGID: SyscallNumber = SyscallNumber(154);
    const GETUID: SyscallNumber = SyscallNumber(174);
    const GETEUID: SyscallNumber = SyscallNumber(175);
    const GETGID: SyscallNumber = SyscallNumber(176);
    const GETEGID: SyscallNumber = SyscallNumber(177);
    const SETSID: SyscallNumber = SyscallNumber(157);
    const GETGROUPS: SyscallNumber = SyscallNumber(158);
    const SYSINFO: SyscallNumber = SyscallNumber(179);
    const SETGID: SyscallNumber = SyscallNumber(144);
    const SETREGID: SyscallNumber = SyscallNumber(143);
    const SETREUID: SyscallNumber = SyscallNumber(145);
    const SETUID: SyscallNumber = SyscallNumber(146);
    const SETRESUID: SyscallNumber = SyscallNumber(147);
    const GETRESUID: SyscallNumber = SyscallNumber(148);
    const SETRESGID: SyscallNumber = SyscallNumber(149);
    const GETRESGID: SyscallNumber = SyscallNumber(150);

    // 线程/同步/信号 / threads, sync, signals
    const FUTEX: SyscallNumber = SyscallNumber(98);
    const RT_SIGACTION: SyscallNumber = SyscallNumber(134);
    const RT_SIGPROCMASK: SyscallNumber = SyscallNumber(135);
    const RT_SIGPENDING: SyscallNumber = SyscallNumber(136);
    const RT_SIGTIMEDWAIT: SyscallNumber = SyscallNumber(137);
    const RT_SIGSUSPEND: SyscallNumber = SyscallNumber(133);
    const RT_SIGRETURN: SyscallNumber = SyscallNumber(139);
    const TKILL: SyscallNumber = SyscallNumber(130);
    const TGKILL: SyscallNumber = SyscallNumber(131);
    const SET_TID_ADDRESS: SyscallNumber = SyscallNumber(96);
    const SET_ROBUST_LIST: SyscallNumber = SyscallNumber(99);
    const GET_ROBUST_LIST: SyscallNumber = SyscallNumber(100);

    // 其它常用 / other common syscalls
    const GETRANDOM: SyscallNumber = SyscallNumber(278);
    const GETITIMER: SyscallNumber = SyscallNumber(102);
    const SETITIMER: SyscallNumber = SyscallNumber(103);
    const GETRLIMIT: SyscallNumber = SyscallNumber(163);
    const GETRUSAGE: SyscallNumber = SyscallNumber(165);
    const SETRLIMIT: SyscallNumber = SyscallNumber(164);
    const UMASK: SyscallNumber = SyscallNumber(166);
    const PRLIMIT64: SyscallNumber = SyscallNumber(261);
    const NANOSLEEP: SyscallNumber = SyscallNumber(101);
    const SYSLOG: SyscallNumber = SyscallNumber(116);

    // Socket / 网络
    const SOCKET: SyscallNumber = SyscallNumber(198);
    const SOCKETPAIR: SyscallNumber = SyscallNumber(199);
    const BIND: SyscallNumber = SyscallNumber(200);
    const LISTEN: SyscallNumber = SyscallNumber(201);
    const ACCEPT: SyscallNumber = SyscallNumber(202);
    const ACCEPT4: SyscallNumber = SyscallNumber(242);
    const CONNECT: SyscallNumber = SyscallNumber(203);
    const GETSOCKNAME: SyscallNumber = SyscallNumber(204);
    const GETPEERNAME: SyscallNumber = SyscallNumber(205);
    const SENDTO: SyscallNumber = SyscallNumber(206);
    const RECVFROM: SyscallNumber = SyscallNumber(207);
    const SENDMSG: SyscallNumber = SyscallNumber(211);
    const RECVMSG: SyscallNumber = SyscallNumber(212);
    const SETSOCKOPT: SyscallNumber = SyscallNumber(208);
    const GETSOCKOPT: SyscallNumber = SyscallNumber(209);
    const SHUTDOWN: SyscallNumber = SyscallNumber(210);
    const POLL: SyscallNumber = SyscallNumber(271);
    const EPOLL_CREATE1: SyscallNumber = SyscallNumber(20);
    const EPOLL_CTL: SyscallNumber = SyscallNumber(21);
    const EPOLL_WAIT: SyscallNumber = SyscallNumber(22);
    const EPOLL_PWAIT: SyscallNumber = SyscallNumber(281);
}

const _: () = assert!(LinuxGeneric64::SELECT.raw() != LinuxGeneric64::DUP.raw());

#[cfg(test)]
mod tests {
    use super::LinuxGeneric64;
    use api_v0::syscall_number::SyscallNumberTable;

    /// 号表中除 `SELECT` 哨兵外，任意两项不得共用同一裸编号。
    #[test]
    fn dispatched_syscall_numbers_are_unique() {
        let nums = [
            LinuxGeneric64::READ,
            LinuxGeneric64::READV,
            LinuxGeneric64::WRITE,
            LinuxGeneric64::WRITEV,
            LinuxGeneric64::PREAD64,
            LinuxGeneric64::PWRITE64,
            LinuxGeneric64::PREADV,
            LinuxGeneric64::PWRITEV,
            LinuxGeneric64::SENDFILE,
            LinuxGeneric64::PSELECT6,
            LinuxGeneric64::PPOLL,
            LinuxGeneric64::READLINKAT,
            LinuxGeneric64::FACCESSAT,
            LinuxGeneric64::FCHMODAT,
            LinuxGeneric64::FCHOWNAT,
            LinuxGeneric64::STATFS,
            LinuxGeneric64::SYNC,
            LinuxGeneric64::FSYNC,
            LinuxGeneric64::FDATASYNC,
            LinuxGeneric64::FTRUNCATE,
            LinuxGeneric64::FALLOCATE,
            LinuxGeneric64::OPENAT,
            LinuxGeneric64::SETXATTR,
            LinuxGeneric64::LSETXATTR,
            LinuxGeneric64::FSETXATTR,
            LinuxGeneric64::GETXATTR,
            LinuxGeneric64::LGETXATTR,
            LinuxGeneric64::FGETXATTR,
            LinuxGeneric64::LISTXATTR,
            LinuxGeneric64::LLISTXATTR,
            LinuxGeneric64::FLISTXATTR,
            LinuxGeneric64::REMOVEXATTR,
            LinuxGeneric64::LREMOVEXATTR,
            LinuxGeneric64::FREMOVEXATTR,
            LinuxGeneric64::CLOSE,
            LinuxGeneric64::FSTAT,
            LinuxGeneric64::LSEEK,
            LinuxGeneric64::DUP,
            LinuxGeneric64::DUP3,
            LinuxGeneric64::PIPE2,
            LinuxGeneric64::IOCTL,
            LinuxGeneric64::FCNTL,
            LinuxGeneric64::GETDENTS64,
            LinuxGeneric64::MKDIRAT,
            LinuxGeneric64::SYMLINKAT,
            LinuxGeneric64::UNLINKAT,
            LinuxGeneric64::RENAMEAT2,
            LinuxGeneric64::UTIMENSAT,
            LinuxGeneric64::MOUNT,
            LinuxGeneric64::UMOUNT2,
            LinuxGeneric64::EXIT,
            LinuxGeneric64::EXIT_GROUP,
            LinuxGeneric64::FORK,
            LinuxGeneric64::CLONE3,
            LinuxGeneric64::WAITPID,
            LinuxGeneric64::KILL,
            LinuxGeneric64::EXEC,
            LinuxGeneric64::SCHED_SETPARAM,
            LinuxGeneric64::SCHED_SETSCHEDULER,
            LinuxGeneric64::SCHED_GETSCHEDULER,
            LinuxGeneric64::SCHED_GETPARAM,
            LinuxGeneric64::SCHED_SETAFFINITY,
            LinuxGeneric64::SCHED_GETAFFINITY,
            LinuxGeneric64::YIELD,
            LinuxGeneric64::GET_TIME,
            LinuxGeneric64::CLOCK_SETTIME,
            LinuxGeneric64::CLOCK_GETTIME,
            LinuxGeneric64::CLOCK_GETRES,
            LinuxGeneric64::CLOCK_NANOSLEEP,
            LinuxGeneric64::BRK,
            LinuxGeneric64::MMAP,
            LinuxGeneric64::MUNMAP,
            LinuxGeneric64::MSYNC,
            LinuxGeneric64::MPROTECT,
            LinuxGeneric64::MREMAP,
            LinuxGeneric64::MADVISE,
            LinuxGeneric64::MLOCK,
            LinuxGeneric64::MUNLOCK,
            LinuxGeneric64::MLOCKALL,
            LinuxGeneric64::MUNLOCKALL,
            LinuxGeneric64::GET_MEMPOLICY,
            LinuxGeneric64::SHMGET,
            LinuxGeneric64::SHMCTL,
            LinuxGeneric64::SHMAT,
            LinuxGeneric64::SHMDT,
            LinuxGeneric64::UNAME,
            LinuxGeneric64::PRCTL,
            LinuxGeneric64::CAPGET,
            LinuxGeneric64::CAPSET,
            LinuxGeneric64::GETPID,
            LinuxGeneric64::GETPPID,
            LinuxGeneric64::GETCWD,
            LinuxGeneric64::CHDIR,
            LinuxGeneric64::GETTID,
            LinuxGeneric64::TIMES,
            LinuxGeneric64::SETPGID,
            LinuxGeneric64::GETUID,
            LinuxGeneric64::GETEUID,
            LinuxGeneric64::GETGID,
            LinuxGeneric64::GETEGID,
            LinuxGeneric64::SETSID,
            LinuxGeneric64::GETGROUPS,
            LinuxGeneric64::SYSINFO,
            LinuxGeneric64::SETGID,
            LinuxGeneric64::SETREGID,
            LinuxGeneric64::SETREUID,
            LinuxGeneric64::SETUID,
            LinuxGeneric64::SETRESUID,
            LinuxGeneric64::GETRESUID,
            LinuxGeneric64::SETRESGID,
            LinuxGeneric64::GETRESGID,
            LinuxGeneric64::FUTEX,
            LinuxGeneric64::RT_SIGACTION,
            LinuxGeneric64::RT_SIGPROCMASK,
            LinuxGeneric64::RT_SIGPENDING,
            LinuxGeneric64::RT_SIGTIMEDWAIT,
            LinuxGeneric64::RT_SIGSUSPEND,
            LinuxGeneric64::RT_SIGRETURN,
            LinuxGeneric64::TKILL,
            LinuxGeneric64::TGKILL,
            LinuxGeneric64::SET_TID_ADDRESS,
            LinuxGeneric64::SET_ROBUST_LIST,
            LinuxGeneric64::GET_ROBUST_LIST,
            LinuxGeneric64::GETRANDOM,
            LinuxGeneric64::GETITIMER,
            LinuxGeneric64::SETITIMER,
            LinuxGeneric64::GETRLIMIT,
            LinuxGeneric64::GETRUSAGE,
            LinuxGeneric64::SETRLIMIT,
            LinuxGeneric64::UMASK,
            LinuxGeneric64::PRLIMIT64,
            LinuxGeneric64::NANOSLEEP,
            LinuxGeneric64::SYSLOG,
            LinuxGeneric64::SOCKET,
            LinuxGeneric64::SOCKETPAIR,
            LinuxGeneric64::BIND,
            LinuxGeneric64::LISTEN,
            LinuxGeneric64::ACCEPT,
            LinuxGeneric64::ACCEPT4,
            LinuxGeneric64::CONNECT,
            LinuxGeneric64::GETSOCKNAME,
            LinuxGeneric64::GETPEERNAME,
            LinuxGeneric64::SENDTO,
            LinuxGeneric64::RECVFROM,
            LinuxGeneric64::SENDMSG,
            LinuxGeneric64::RECVMSG,
            LinuxGeneric64::SETSOCKOPT,
            LinuxGeneric64::GETSOCKOPT,
            LinuxGeneric64::SHUTDOWN,
            LinuxGeneric64::POLL,
            LinuxGeneric64::EPOLL_CREATE1,
            LinuxGeneric64::EPOLL_CTL,
            LinuxGeneric64::EPOLL_WAIT,
            LinuxGeneric64::EPOLL_PWAIT,
        ]
        .map(|n| n.raw());

        for (i, left) in nums.iter().enumerate() {
            for (j, right) in nums.iter().enumerate().skip(i + 1) {
                assert_ne!(
                    left, right,
                    "syscall number collision: index {i} and {j} both use {left}"
                );
            }
        }
    }
}
