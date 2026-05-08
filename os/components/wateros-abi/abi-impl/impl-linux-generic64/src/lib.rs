#![no_std]

use api_v0::syscall_number::{SyscallNumber, SyscallNumberTable};

/// Linux asm-generic 64-bit syscall 号表（早期阶段维护 busybox /
/// 简单进程子集）。
///
/// RISC-V 64 与 LoongArch64 当前都通过 `wateros-abi` 的架构 feature
/// 选择这张表； 后续若架构产生差异，再拆分专用 impl。
pub struct LinuxGeneric64;

impl SyscallNumberTable for LinuxGeneric64 {
    // I/O
    const READ : SyscallNumber = SyscallNumber(63);
    const WRITE : SyscallNumber = SyscallNumber(64);
    const OPENAT : SyscallNumber = SyscallNumber(56);
    const CLOSE : SyscallNumber = SyscallNumber(57);
    const FSTAT : SyscallNumber = SyscallNumber(80);
    const LSEEK : SyscallNumber = SyscallNumber(62);
    const DUP : SyscallNumber = SyscallNumber(23);
    const DUP3 : SyscallNumber = SyscallNumber(24);
    const PIPE2 : SyscallNumber = SyscallNumber(59);
    const IOCTL : SyscallNumber = SyscallNumber(29);
    const FCNTL : SyscallNumber = SyscallNumber(25);

    // 进程/执行
    const EXIT : SyscallNumber = SyscallNumber(93);
    const EXIT_GROUP : SyscallNumber = SyscallNumber(94);
    const FORK : SyscallNumber = SyscallNumber(220); // 常见用户态 fork -> clone
    const WAITPID : SyscallNumber = SyscallNumber(260); // wait4
    const EXEC : SyscallNumber = SyscallNumber(221); // execve

    // 调度/时间
    const YIELD : SyscallNumber = SyscallNumber(124); // sched_yield
    const GET_TIME : SyscallNumber = SyscallNumber(169); // gettimeofday
    const CLOCK_GETTIME : SyscallNumber = SyscallNumber(113); // clock_gettime

    // 内存管理
    const BRK : SyscallNumber = SyscallNumber(214); // brk
    const MMAP : SyscallNumber = SyscallNumber(222); // mmap
    const MUNMAP : SyscallNumber = SyscallNumber(215); // munmap
    const MPROTECT : SyscallNumber = SyscallNumber(226); // mprotect

    // 基本信息
    const UNAME : SyscallNumber = SyscallNumber(160); // uname
    const PRCTL : SyscallNumber = SyscallNumber(167); // prctl
    const GETPID : SyscallNumber = SyscallNumber(172);
    const GETCWD : SyscallNumber = SyscallNumber(17);
    const GETTID : SyscallNumber = SyscallNumber(178);

    // 线程/同步/信号
    const FUTEX : SyscallNumber = SyscallNumber(98);
    const RT_SIGACTION : SyscallNumber = SyscallNumber(134);
    const RT_SIGPROCMASK : SyscallNumber = SyscallNumber(135);
    const RT_SIGRETURN : SyscallNumber = SyscallNumber(139);
    const SET_TID_ADDRESS : SyscallNumber = SyscallNumber(96);
    const SET_ROBUST_LIST : SyscallNumber = SyscallNumber(99);

    // 其它常用
    const GETRANDOM : SyscallNumber = SyscallNumber(278);
    const SETITIMER : SyscallNumber = SyscallNumber(103);
    const GETRLIMIT : SyscallNumber = SyscallNumber(163);
    const SETRLIMIT : SyscallNumber = SyscallNumber(164);
    const NANOSLEEP : SyscallNumber = SyscallNumber(101);
}
