//! 系统调用编号的类型抽象，以及按 libc ABI 对齐的调用号表接口。
//!
//! 表项仅表达“编号映射”，不包含内核是否已实现某调用的能力声明。
//!
//! English: maps symbolic syscall names to numeric IDs for libc-aligned dispatch;
//! presence in the table does not imply the kernel implements the syscall.

/// Linux/riscv64 系统调用号 newtype（只表示编号，不在类型层做合法性校验）。
///
/// English: opaque syscall number carrier; does not prove the ID is implemented.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SyscallNumber(
    /// 平台 ABI 下的裸系统调用编号。
    pub usize,
);

impl SyscallNumber {
    /// 由裸编号构造；不检查该编号在当前内核是否受支持。
    ///
    /// English: wraps a raw syscall number without capability checks.
    #[inline]
    pub const fn new(n: usize) -> Self {
        Self(n)
    }

    /// 取底层 `usize` 调用号。
    ///
    /// English: exposes the underlying numeric syscall ID.
    #[inline]
    pub const fn raw(self) -> usize {
        self.0
    }
}

/// 系统调用号表：用于在 `syscall` 分发层把“syscall 号 -> handler”对齐到 libc ABI。
///
/// 目前处于早期阶段，所以只维护“跑 busybox / 简单进程”所需的常用子集；
/// 后续按 strace/缺口逐步补齐即可。
///
/// English: associates symbolic names with libc syscall numbers; early subset only,
/// extend as userland coverage grows.
pub trait SyscallNumberTable {
    // I/O / 文件与描述符相关
    /// 从打开对象读数据（与 `read(2)` 语义对齐的编号）。
    const READ: SyscallNumber;
    /// 向打开对象写数据（与 `write(2)` 语义对齐的编号）。
    const WRITE: SyscallNumber;
    /// 相对目录打开路径（`openat(2)`）。
    const OPENAT: SyscallNumber;
    /// 关闭文件描述符（`close(2)`）。
    const CLOSE: SyscallNumber;
    /// 取打开对象元数据（`fstat`/`newfstatat` 等 libc 路径对应的编号）。
    const FSTAT: SyscallNumber;
    /// 调整读写偏移（`lseek(2)`）。
    const LSEEK: SyscallNumber;
    /// 复制文件描述符（`dup(2)`）。
    const DUP: SyscallNumber;
    /// 在指定最小编号上复制 fd（`dup3(2)`）。
    const DUP3: SyscallNumber;
    /// 创建管道（`pipe2(2)`）。
    const PIPE2: SyscallNumber;
    /// 设备/套接字控制（`ioctl(2)`）。
    const IOCTL: SyscallNumber;
    /// 文件描述符控制（`fcntl(2)`）。
    const FCNTL: SyscallNumber;

    // 进程/执行 / process & execution
    /// 终止当前线程（`exit(2)`）。
    const EXIT: SyscallNumber;
    /// 终止进程内全部线程（`exit_group(2)`）。
    const EXIT_GROUP: SyscallNumber;
    /// 创建子进程；用户态常映射为 `clone` 族调用。
    const FORK: SyscallNumber;
    /// 等待子进程状态变更（与 `wait4` 等价的编号）。
    const WAITPID: SyscallNumber;
    /// 替换当前进程映像（与 `execve` 等价的编号）。
    const EXEC: SyscallNumber;

    // 调度/时间 / scheduling & time
    /// 主动让出 CPU（`sched_yield(2)`）。
    const YIELD: SyscallNumber;
    /// 取墙上时钟（`gettimeofday(2)`）。
    const GET_TIME: SyscallNumber;
    /// 取指定时钟源时间（`clock_gettime(2)`）。
    const CLOCK_GETTIME: SyscallNumber;

    // 内存管理（glibc/musl 常见）/ memory management
    /// 调整 program break（`brk(2)`）。
    const BRK: SyscallNumber;
    /// 建立内存映射（`mmap(2)`）。
    const MMAP: SyscallNumber;
    /// 解除映射（`munmap(2)`）。
    const MUNMAP: SyscallNumber;
    /// 修改映射保护属性（`mprotect(2)`）。
    const MPROTECT: SyscallNumber;

    // 基本信息 / identity & misc info
    /// 内核与体系结构标识（`uname(2)`）。
    const UNAME: SyscallNumber;
    /// 进程控制杂项（`prctl(2)`）。
    const PRCTL: SyscallNumber;
    /// 当前进程 ID（`getpid(2)`）。
    const GETPID: SyscallNumber;
    /// 取当前工作目录（`getcwd(2)`）。
    const GETCWD: SyscallNumber;
    /// 当前线程 ID（`gettid(2)`）。
    const GETTID: SyscallNumber;

    // 线程/同步/信号（glibc/musl 常见）/ threads, sync, signals
    /// 用户态快速互斥与睡眠原语（`futex(2)`）。
    const FUTEX: SyscallNumber;
    /// 安装信号处理函数（`rt_sigaction(2)`）。
    const RT_SIGACTION: SyscallNumber;
    /// 阻塞/解除阻塞信号集（`rt_sigprocmask(2)`）。
    const RT_SIGPROCMASK: SyscallNumber;
    /// 从信号处理返回（`rt_sigreturn(2)`）。
    const RT_SIGRETURN: SyscallNumber;
    /// 设置 clear_child_tid 地址（`set_tid_address(2)`）。
    const SET_TID_ADDRESS: SyscallNumber;
    /// robust futex 列表（`set_robust_list(2)`）。
    const SET_ROBUST_LIST: SyscallNumber;

    // 其它常用（早期阶段优先级不高，但常见）/ other common syscalls
    /// 从内核熵池取随机字节（`getrandom(2)`）。
    const GETRANDOM: SyscallNumber;
    /// 设置间隔定时器（`setitimer(2)`）。
    const SETITIMER: SyscallNumber;
    /// 查询资源软/硬上限（`getrlimit(2)`）。
    const GETRLIMIT: SyscallNumber;
    /// 设置资源软/硬上限（`setrlimit(2)`）。
    const SETRLIMIT: SyscallNumber;
    /// 可中断的纳秒级睡眠（`nanosleep(2)`）。
    const NANOSLEEP: SyscallNumber;
}
