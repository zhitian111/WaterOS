#![no_std]
//! Linux riscv64 上与 libc 对齐的系统调用号具体取值。
//!
//! 变更时需与用户态工具链、内核分发及常见 `strace` 输出保持一致。

use api_v0::syscall_number::{SyscallNumber, SyscallNumberTable};

/// 提供 [`SyscallNumberTable`] 的 Linux riscv64 常量表（当前为 busybox / 简单进程所需子集）。
pub struct LinuxRiscv64;

impl SyscallNumberTable for LinuxRiscv64 {
    // I/O
    const READ: SyscallNumber = SyscallNumber(63);
    const WRITE: SyscallNumber = SyscallNumber(64);
    const OPENAT: SyscallNumber = SyscallNumber(56);
    const CLOSE: SyscallNumber = SyscallNumber(57);
    const FSTAT: SyscallNumber = SyscallNumber(80);
    const LSEEK: SyscallNumber = SyscallNumber(62);
    const DUP: SyscallNumber = SyscallNumber(23);
    const DUP3: SyscallNumber = SyscallNumber(24);
    const PIPE2: SyscallNumber = SyscallNumber(59);
    const IOCTL: SyscallNumber = SyscallNumber(29);
    const FCNTL: SyscallNumber = SyscallNumber(25);

    // 进程/执行
    const EXIT: SyscallNumber = SyscallNumber(93);
    const EXIT_GROUP: SyscallNumber = SyscallNumber(94);
    const FORK: SyscallNumber = SyscallNumber(220); // 常见用户态 fork -> clone
    const WAITPID: SyscallNumber = SyscallNumber(260); // wait4
    const EXEC: SyscallNumber = SyscallNumber(221); // execve

    // 调度/时间
    const YIELD: SyscallNumber = SyscallNumber(124); // sched_yield
    const GET_TIME: SyscallNumber = SyscallNumber(169); // gettimeofday
    const CLOCK_GETTIME: SyscallNumber = SyscallNumber(113); // clock_gettime

    // 内存管理
    const BRK: SyscallNumber = SyscallNumber(214); // brk
    const MMAP: SyscallNumber = SyscallNumber(222); // mmap
    const MUNMAP: SyscallNumber = SyscallNumber(215); // munmap
    const MPROTECT: SyscallNumber = SyscallNumber(226); // mprotect

    // 基本信息
    const UNAME: SyscallNumber = SyscallNumber(160); // uname
    const PRCTL: SyscallNumber = SyscallNumber(167); // prctl
    const GETPID: SyscallNumber = SyscallNumber(172);
    const GETCWD: SyscallNumber = SyscallNumber(17);
    const GETTID: SyscallNumber = SyscallNumber(178);

    // 线程/同步/信号
    const FUTEX: SyscallNumber = SyscallNumber(98);
    const RT_SIGACTION: SyscallNumber = SyscallNumber(134);
    const RT_SIGPROCMASK: SyscallNumber = SyscallNumber(135);
    const RT_SIGRETURN: SyscallNumber = SyscallNumber(139);
    const SET_TID_ADDRESS: SyscallNumber = SyscallNumber(96);
    const SET_ROBUST_LIST: SyscallNumber = SyscallNumber(99);

    // 其它常用
    const GETRANDOM: SyscallNumber = SyscallNumber(278);
    const SETITIMER: SyscallNumber = SyscallNumber(103);
    const GETRLIMIT: SyscallNumber = SyscallNumber(163);
    const SETRLIMIT: SyscallNumber = SyscallNumber(164);
    const NANOSLEEP: SyscallNumber = SyscallNumber(101);
}

/// 兼容旧名字：glibc 与 musl 在 riscv64 + Linux 上复用同一套 syscall 编号表
pub type Glibc = LinuxRiscv64;
pub type Musl = LinuxRiscv64;
