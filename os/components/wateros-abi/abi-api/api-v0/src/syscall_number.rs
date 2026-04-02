/// Linux/riscv64 系统调用号 newtype（只表示编号，不在类型层做合法性校验）
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SyscallNumber(pub usize);

impl SyscallNumber {
    #[inline]
    pub const fn new(n: usize) -> Self {
        Self(n)
    }

    #[inline]
    pub const fn raw(self) -> usize {
        self.0
    }
}

/// 系统调用号表：用于在 `syscall` 分发层把“syscall 号 -> handler”对齐到 libc ABI。
///
/// 目前处于早期阶段，所以只维护“跑 busybox / 简单进程”所需的常用子集；
/// 后续按 strace/缺口逐步补齐即可。
pub trait SyscallNumberTable {
    // I/O
    const READ: SyscallNumber;
    const WRITE: SyscallNumber;
    const OPENAT: SyscallNumber;
    const CLOSE: SyscallNumber;
    const FSTAT: SyscallNumber;
    const LSEEK: SyscallNumber;
    const DUP: SyscallNumber;
    const DUP3: SyscallNumber;
    const PIPE2: SyscallNumber;
    const IOCTL: SyscallNumber;
    const FCNTL: SyscallNumber;

    // 进程/执行
    const EXIT: SyscallNumber;
    const EXIT_GROUP: SyscallNumber;
    const FORK: SyscallNumber; // 实际可能由 clone 实现
    const WAITPID: SyscallNumber; // wait4 对应
    const EXEC: SyscallNumber; // execve 对应

    // 调度/时间
    const YIELD: SyscallNumber; // sched_yield
    const GET_TIME: SyscallNumber; // gettimeofday
    const CLOCK_GETTIME: SyscallNumber;

    // 内存管理（glibc/musl 常见）
    const BRK: SyscallNumber;
    const MMAP: SyscallNumber;
    const MUNMAP: SyscallNumber;
    const MPROTECT: SyscallNumber;

    // 基本信息
    const UNAME: SyscallNumber;
    const PRCTL: SyscallNumber;
    const GETPID: SyscallNumber;
    const GETCWD: SyscallNumber;
    const GETTID: SyscallNumber;

    // 线程/同步/信号（glibc/musl 常见）
    const FUTEX: SyscallNumber;
    const RT_SIGACTION: SyscallNumber;
    const RT_SIGPROCMASK: SyscallNumber;
    const RT_SIGRETURN: SyscallNumber;
    const SET_TID_ADDRESS: SyscallNumber;
    const SET_ROBUST_LIST: SyscallNumber;

    // 其它常用（早期阶段优先级不高，但常见）
    const GETRANDOM: SyscallNumber;
    const SETITIMER: SyscallNumber;
    const GETRLIMIT: SyscallNumber;
    const SETRLIMIT: SyscallNumber;
    const NANOSLEEP: SyscallNumber;
}
