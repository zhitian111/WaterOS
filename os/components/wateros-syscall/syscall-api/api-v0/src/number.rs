//! Linux generic 64 位系统调用号及其类型安全包装。

/// 裸系统调用号的透明包装。
///
/// 该类型只区分“系统调用号”和普通 `usize`，不保证当前内核实现了该编号。
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SyscallNumber(pub usize);

impl SyscallNumber {
    /// 由裸编号构造。
    #[inline]
    pub const fn new(number : usize) -> Self { Self(number) }

    /// 取出底层编号。
    #[inline]
    pub const fn raw(self) -> usize { self.0 }
}

// 文件与描述符
pub const READ : usize = 63;
pub const READV : usize = 65;
pub const WRITE : usize = 64;
pub const WRITEV : usize = 66;
pub const PREAD64 : usize = 67;
pub const PWRITE64 : usize = 68;
pub const PREADV : usize = 69;
pub const PWRITEV : usize = 70;
pub const SENDFILE : usize = 71;
pub const FADVISE64 : usize = 223;
pub const PSELECT6 : usize = 72;
pub const PPOLL : usize = 73;
/// asm-generic 64 位无独立 `select` nr；riscv64/loong64 用户态经 `pselect6`(72) 实现。
pub const SELECT : usize = usize::MAX;
pub const READLINKAT : usize = 78;
pub const FACCESSAT : usize = 48;
pub const FCHDIR : usize = 50;
pub const FCHMOD : usize = 52;
pub const FCHMODAT : usize = 53;
pub const FCHOWN : usize = 55;
pub const FCHOWNAT : usize = 54;
pub const STATFS : usize = 43;
pub const SYNC : usize = 81;
pub const FSYNC : usize = 82;
pub const FDATASYNC : usize = 83;
pub const TRUNCATE : usize = 45;
pub const FTRUNCATE : usize = 46;
pub const FALLOCATE : usize = 47;
pub const OPENAT : usize = 56;
pub const SETXATTR : usize = 5;
pub const LSETXATTR : usize = 6;
pub const FSETXATTR : usize = 7;
pub const GETXATTR : usize = 8;
pub const LGETXATTR : usize = 9;
pub const FGETXATTR : usize = 10;
pub const LISTXATTR : usize = 11;
pub const LLISTXATTR : usize = 12;
pub const FLISTXATTR : usize = 13;
pub const REMOVEXATTR : usize = 14;
pub const LREMOVEXATTR : usize = 15;
pub const FREMOVEXATTR : usize = 16;
pub const CLOSE : usize = 57;
pub const FSTAT : usize = 80;
pub const LSEEK : usize = 62;
pub const DUP : usize = 23;
pub const DUP3 : usize = 24;
pub const PIPE2 : usize = 59;
pub const EVENTFD2 : usize = 19;
pub const IOCTL : usize = 29;
pub const FCNTL : usize = 25;
pub const FLOCK : usize = 32;
pub const GETDENTS64 : usize = 61;
pub const MKDIRAT : usize = 34;
pub const SYMLINKAT : usize = 36;
pub const UNLINKAT : usize = 35;
pub const RENAMEAT : usize = 38;
pub const RENAMEAT2 : usize = 276;
pub const UTIMENSAT : usize = 88;
pub const MOUNT : usize = 40;
pub const UMOUNT2 : usize = 39;

// 进程与执行
pub const EXIT : usize = 93;
pub const EXIT_GROUP : usize = 94;
pub const FORK : usize = 220; // 用户态 fork 常映射到 clone
pub const CLONE3 : usize = 435;
pub const WAITPID : usize = 260; // wait4
pub const WAITID : usize = 95;
pub const KILL : usize = 129;
pub const EXEC : usize = 221; // execve
pub const UNSHARE : usize = 272;

// 调度与时间
pub const SCHED_SETPARAM : usize = 118;
pub const SCHED_SETSCHEDULER : usize = 119;
pub const SCHED_GETSCHEDULER : usize = 120;
pub const SCHED_GETPARAM : usize = 121;
pub const SCHED_SETAFFINITY : usize = 122;
pub const SCHED_GETAFFINITY : usize = 123;
pub const SCHED_GET_PRIORITY_MAX : usize = 125;
pub const SCHED_GET_PRIORITY_MIN : usize = 126;
pub const YIELD : usize = 124;
pub const GETCPU : usize = 168;
pub const GET_TIME : usize = 169;
pub const CLOCK_SETTIME : usize = 112;
pub const CLOCK_GETTIME : usize = 113;
pub const CLOCK_GETRES : usize = 114;
pub const CLOCK_NANOSLEEP : usize = 115;
pub const TIMER_CREATE : usize = 107;
pub const TIMER_GETTIME : usize = 108;
pub const TIMER_GETOVERRUN : usize = 109;
pub const TIMER_SETTIME : usize = 110;
pub const TIMER_DELETE : usize = 111;

// 内存管理
pub const BRK : usize = 214;
pub const MMAP : usize = 222;
pub const MUNMAP : usize = 215;
pub const MSYNC : usize = 227;
pub const MPROTECT : usize = 226;
pub const MREMAP : usize = 216;
pub const MADVISE : usize = 233;
pub const MLOCK : usize = 228;
pub const MUNLOCK : usize = 229;
pub const MLOCKALL : usize = 230;
pub const MUNLOCKALL : usize = 231;
pub const GET_MEMPOLICY : usize = 236;
pub const SHMGET : usize = 194;
pub const SHMCTL : usize = 195;
pub const SHMAT : usize = 196;
pub const SHMDT : usize = 197;

// 进程标识与杂项信息
pub const UNAME : usize = 160;
pub const PRCTL : usize = 167;
pub const CAPGET : usize = 90;
pub const CAPSET : usize = 91;
pub const GETPID : usize = 172;
pub const GETPPID : usize = 173;
pub const GETCWD : usize = 17;
pub const CHDIR : usize = 49;
pub const GETTID : usize = 178;
pub const TIMES : usize = 153;
pub const SETPGID : usize = 154;
pub const GETPGID : usize = 155;
pub const SETPRIORITY : usize = 140;
pub const GETPRIORITY : usize = 141;
pub const GETUID : usize = 174;
pub const GETEUID : usize = 175;
pub const GETGID : usize = 176;
pub const GETEGID : usize = 177;
pub const SETSID : usize = 157;
pub const GETSID : usize = 156;
pub const GETGROUPS : usize = 158;
pub const SYSINFO : usize = 179;
pub const SETGID : usize = 144;
pub const SETREGID : usize = 143;
pub const SETREUID : usize = 145;
pub const SETUID : usize = 146;
pub const SETRESUID : usize = 147;
pub const GETRESUID : usize = 148;
pub const SETRESGID : usize = 149;
pub const GETRESGID : usize = 150;

// 线程、同步与信号
pub const FUTEX : usize = 98;
pub const RT_SIGACTION : usize = 134;
pub const SIGALTSTACK : usize = 132;
pub const RT_SIGPROCMASK : usize = 135;
pub const RT_SIGPENDING : usize = 136;
pub const RT_SIGTIMEDWAIT : usize = 137;
pub const RT_SIGSUSPEND : usize = 133;
pub const RT_SIGRETURN : usize = 139;
pub const TKILL : usize = 130;
pub const TGKILL : usize = 131;
pub const SET_TID_ADDRESS : usize = 96;
pub const SET_ROBUST_LIST : usize = 99;
pub const GET_ROBUST_LIST : usize = 100;
pub const RSEQ : usize = 293;
/// RISC-V architecture-specific hardware probing syscall.
pub const RISCV_HWPROBE : usize = 258;
/// RISC-V architecture-specific instruction-cache synchronization syscall.
pub const RISCV_FLUSH_ICACHE : usize = 259;

// 其它常用调用
pub const GETRANDOM : usize = 278;
pub const GETITIMER : usize = 102;
pub const SETITIMER : usize = 103;
pub const GETRLIMIT : usize = 163;
pub const GETRUSAGE : usize = 165;
pub const SETRLIMIT : usize = 164;
pub const UMASK : usize = 166;
pub const PRLIMIT64 : usize = 261;
pub const NANOSLEEP : usize = 101;
pub const SYSLOG : usize = 116;

// Socket 与网络
pub const SOCKET : usize = 198;
pub const SOCKETPAIR : usize = 199;
pub const BIND : usize = 200;
pub const LISTEN : usize = 201;
pub const ACCEPT : usize = 202;
pub const ACCEPT4 : usize = 242;
pub const CONNECT : usize = 203;
pub const GETSOCKNAME : usize = 204;
pub const GETPEERNAME : usize = 205;
pub const SENDTO : usize = 206;
pub const RECVFROM : usize = 207;
pub const SENDMSG : usize = 211;
pub const RECVMSG : usize = 212;
pub const SENDMMSG : usize = 269;
pub const SETSOCKOPT : usize = 208;
pub const GETSOCKOPT : usize = 209;
pub const SHUTDOWN : usize = 210;
/// asm-generic 64 位无独立 `poll` nr；riscv64/loong64 用户态经 `ppoll`(73) 实现。
pub const POLL : usize = usize::MAX;
pub const EPOLL_CREATE1 : usize = 20;
pub const EPOLL_CTL : usize = 21;
/// asm-generic 64 位无独立 `epoll_wait` nr；riscv64/loong64 用户态经 `epoll_pwait`(22) 实现。
pub const EPOLL_WAIT : usize = usize::MAX;
pub const EPOLL_PWAIT : usize = 22;
pub const EPOLL_PWAIT2 : usize = 441;

// 别名/兼容 syscall – 已在别处实现，此处仅定义号段
pub const FSTATAT : usize = 79;
pub const STATX : usize = 291;
pub const SCHED_SETATTR : usize = 274;
pub const SCHED_GETATTR : usize = 275;
pub const SCHED_RR_GET_INTERVAL : usize = 127;
pub const FACESSAT2 : usize = 439;
pub const ADJTIMEX : usize = 171;
pub const CLOCK_ADJTIME : usize = 266;
pub const ACCT : usize = 89;
pub const CLOSE_RANGE : usize = 436;
pub const SETGROUPS : usize = 159;
pub const FSTATFS : usize = 44;
pub const LINKAT : usize = 37;
pub const MKNODAT : usize = 33;
