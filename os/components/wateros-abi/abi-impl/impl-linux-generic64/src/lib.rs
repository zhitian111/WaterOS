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
    const SELECT: SyscallNumber = SyscallNumber(23);
    const READLINKAT: SyscallNumber = SyscallNumber(78);
    const FACCESSAT: SyscallNumber = SyscallNumber(48);
    const STATFS: SyscallNumber = SyscallNumber(43);
    const SYNC: SyscallNumber = SyscallNumber(81);
    const FSYNC: SyscallNumber = SyscallNumber(82);
    const FDATASYNC: SyscallNumber = SyscallNumber(83);
    const FTRUNCATE: SyscallNumber = SyscallNumber(46);
    const OPENAT: SyscallNumber = SyscallNumber(56);
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
    const GET_MEMPOLICY: SyscallNumber = SyscallNumber(236); // get_mempolicy
    const SHMGET: SyscallNumber = SyscallNumber(194);
    const SHMCTL: SyscallNumber = SyscallNumber(195);
    const SHMAT: SyscallNumber = SyscallNumber(196);
    const SHMDT: SyscallNumber = SyscallNumber(197);

    // 基本信息 / identity & misc info
    const UNAME: SyscallNumber = SyscallNumber(160); // uname
    const PRCTL: SyscallNumber = SyscallNumber(167); // prctl
    const GETPID: SyscallNumber = SyscallNumber(172);
    const GETPPID: SyscallNumber = SyscallNumber(173);
    const GETCWD: SyscallNumber = SyscallNumber(17);
    const CHDIR: SyscallNumber = SyscallNumber(49);
    const GETTID: SyscallNumber = SyscallNumber(178);
    const TIMES: SyscallNumber = SyscallNumber(153);
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
    const SETRESGID: SyscallNumber = SyscallNumber(149);

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
}
