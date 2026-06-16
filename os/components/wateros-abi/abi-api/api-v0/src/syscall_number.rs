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
    /// 从打开对象读入多段缓冲（`readv(2)`）。
    const READV: SyscallNumber;
    /// 向打开对象写数据（与 `write(2)` 语义对齐的编号）。
    const WRITE: SyscallNumber;
    /// 向打开对象写入多段缓冲（`writev(2)`）。
    const WRITEV: SyscallNumber;
    /// 在指定偏移读（`pread64(2)`）。
    const PREAD64: SyscallNumber;
    /// 在指定偏移写（`pwrite64(2)`）。
    const PWRITE64: SyscallNumber;
    /// 在指定偏移分散读（`preadv(2)`）。
    const PREADV: SyscallNumber;
    /// 在指定偏移聚集写（`pwritev(2)`）。
    const PWRITEV: SyscallNumber;
    /// 内核态 fd 间拷贝（`sendfile(2)`）。
    const SENDFILE: SyscallNumber;
    /// 读取符号链接目标（`readlinkat(2)`）。
    const READLINKAT: SyscallNumber;
    /// 检查相对目录路径可访问性（`faccessat(2)`）。
    const FACCESSAT: SyscallNumber;
    /// 查询路径所在文件系统统计信息（`statfs(2)`）。
    const STATFS: SyscallNumber;
    /// 请求所有已挂载文件系统写回脏数据（`sync(2)`）。
    const SYNC: SyscallNumber;
    /// 将 fd 对应文件脏数据同步到存储（`fsync(2)`）。
    const FSYNC: SyscallNumber;
    /// 将 fd 对应文件数据同步到存储（`fdatasync(2)`）。
    const FDATASYNC: SyscallNumber;
    /// 调整 fd 对应文件长度（`ftruncate(2)`）。
    const FTRUNCATE: SyscallNumber;
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
    /// 读取目录项（`getdents64(2)`）。
    const GETDENTS64: SyscallNumber;
    /// 相对目录创建目录（`mkdirat(2)`）。
    const MKDIRAT: SyscallNumber;
    /// 相对目录删除目录项（`unlinkat(2)`）。
    const UNLINKAT: SyscallNumber;
    /// 相对目录重命名目录项（`renameat2(2)`）。
    const RENAMEAT2: SyscallNumber;
    /// 相对目录更新时间戳（`utimensat(2)`）。
    const UTIMENSAT: SyscallNumber;
    /// 挂载文件系统（`mount(2)`）。
    const MOUNT: SyscallNumber;
    /// 卸载文件系统（`umount2(2)`）。
    const UMOUNT2: SyscallNumber;

    // 进程/执行 / process & execution
    /// 终止当前线程（`exit(2)`）。
    const EXIT: SyscallNumber;
    /// 终止进程内全部线程（`exit_group(2)`）。
    const EXIT_GROUP: SyscallNumber;
    /// 创建子进程；用户态常映射为 `clone` 族调用。
    const FORK: SyscallNumber;
    /// 使用结构体参数创建进程/线程（`clone3(2)`）。
    const CLONE3: SyscallNumber;
    /// 等待子进程状态变更（与 `wait4` 等价的编号）。
    const WAITPID: SyscallNumber;
    /// 向进程发送信号（`kill(2)`）。
    const KILL: SyscallNumber;
    /// 替换当前进程映像（与 `execve` 等价的编号）。
    const EXEC: SyscallNumber;

    // 调度/时间 / scheduling & time
    /// 设置调度参数（`sched_setparam(2)`）。
    const SCHED_SETPARAM: SyscallNumber;
    /// 设置调度策略（`sched_setscheduler(2)`）。
    const SCHED_SETSCHEDULER: SyscallNumber;
    /// 查询调度策略（`sched_getscheduler(2)`）。
    const SCHED_GETSCHEDULER: SyscallNumber;
    /// 查询调度参数（`sched_getparam(2)`）。
    const SCHED_GETPARAM: SyscallNumber;
    /// 设置 CPU 亲和性（`sched_setaffinity(2)`）。
    const SCHED_SETAFFINITY: SyscallNumber;
    /// 查询 CPU 亲和性掩码（`sched_getaffinity(2)`）。
    const SCHED_GETAFFINITY: SyscallNumber;
    /// 主动让出 CPU（`sched_yield(2)`）。
    const YIELD: SyscallNumber;
    /// 取墙上时钟（`gettimeofday(2)`）。
    const GET_TIME: SyscallNumber;
    /// 设置指定时钟源时间（`clock_settime(2)`）。
    const CLOCK_SETTIME: SyscallNumber;
    /// 取指定时钟源时间（`clock_gettime(2)`）。
    const CLOCK_GETTIME: SyscallNumber;
    /// 查询时钟分辨率（`clock_getres(2)`）。
    const CLOCK_GETRES: SyscallNumber;
    /// 可指定时钟源的纳秒级睡眠（`clock_nanosleep(2)`）。
    const CLOCK_NANOSLEEP: SyscallNumber;

    // 内存管理（glibc/musl 常见）/ memory management
    /// 调整 program break（`brk(2)`）。
    const BRK: SyscallNumber;
    /// 建立内存映射（`mmap(2)`）。
    const MMAP: SyscallNumber;
    /// 解除映射（`munmap(2)`）。
    const MUNMAP: SyscallNumber;
    /// 同步/失效内存映射页（`msync(2)`）。
    const MSYNC: SyscallNumber;
    /// 修改映射保护属性（`mprotect(2)`）。
    const MPROTECT: SyscallNumber;
    /// 查询 NUMA 内存策略（`get_mempolicy(2)`）。
    const GET_MEMPOLICY: SyscallNumber;

    // 基本信息 / identity & misc info
    /// 内核与体系结构标识（`uname(2)`）。
    const UNAME: SyscallNumber;
    /// 进程控制杂项（`prctl(2)`）。
    const PRCTL: SyscallNumber;
    /// 当前进程 ID（`getpid(2)`）。
    const GETPID: SyscallNumber;
    /// 父进程 ID（`getppid(2)`）。
    const GETPPID: SyscallNumber;
    /// 取当前工作目录（`getcwd(2)`）。
    const GETCWD: SyscallNumber;
    /// 切换当前工作目录（`chdir(2)`）。
    const CHDIR: SyscallNumber;
    /// 当前线程 ID（`gettid(2)`）。
    const GETTID: SyscallNumber;
    /// 读取进程时间统计（`times(2)`）。
    const TIMES: SyscallNumber;
    /// 当前进程真实用户 ID（`getuid(2)`）。
    const GETUID: SyscallNumber;
    /// 当前进程有效用户 ID（`geteuid(2)`）。
    const GETEUID: SyscallNumber;
    /// 当前进程真实组 ID（`getgid(2)`）。
    const GETGID: SyscallNumber;
    /// 当前进程有效组 ID（`getegid(2)`）。
    const GETEGID: SyscallNumber;
    /// 创建新会话（`setsid(2)`）。
    const SETSID: SyscallNumber;
    /// 读取 supplementary 组列表（`getgroups(2)`）。
    const GETGROUPS: SyscallNumber;
    /// 查询系统内存与负载摘要（`sysinfo(2)`）。
    const SYSINFO: SyscallNumber;
    /// 设置真实组 ID（`setgid(2)`）。
    const SETGID: SyscallNumber;
    /// 设置真实/有效组 ID（`setregid(2)`）。
    const SETREGID: SyscallNumber;
    /// 设置真实/有效用户 ID（`setreuid(2)`）。
    const SETREUID: SyscallNumber;
    /// 设置有效用户 ID（`setuid(2)`）。
    const SETUID: SyscallNumber;
    /// 设置 real/effective/saved 用户 ID（`setresuid(2)`）。
    const SETRESUID: SyscallNumber;
    /// 设置 real/effective/saved 组 ID（`setresgid(2)`）。
    const SETRESGID: SyscallNumber;

    // 线程/同步/信号（glibc/musl 常见）/ threads, sync, signals
    /// 用户态快速互斥与睡眠原语（`futex(2)`）。
    const FUTEX: SyscallNumber;
    /// 安装信号处理函数（`rt_sigaction(2)`）。
    const RT_SIGACTION: SyscallNumber;
    /// 阻塞/解除阻塞信号集（`rt_sigprocmask(2)`）。
    const RT_SIGPROCMASK: SyscallNumber;
    /// 查询 pending 信号集（`rt_sigpending(2)`）。
    const RT_SIGPENDING: SyscallNumber;
    /// 临时替换 mask 并等待信号（`rt_sigsuspend(2)`）。
    const RT_SIGSUSPEND: SyscallNumber;
    /// 等待一组实时信号（`rt_sigtimedwait(2)`）。
    const RT_SIGTIMEDWAIT: SyscallNumber;
    /// 从信号处理返回（`rt_sigreturn(2)`）。
    const RT_SIGRETURN: SyscallNumber;
    /// 向指定线程发送信号（`tkill(2)`）。
    const TKILL: SyscallNumber;
    /// 向指定线程组内线程发送信号（`tgkill(2)`）。
    const TGKILL: SyscallNumber;
    /// 设置 clear_child_tid 地址（`set_tid_address(2)`）。
    const SET_TID_ADDRESS: SyscallNumber;
    /// robust futex 列表（`set_robust_list(2)`）。
    const SET_ROBUST_LIST: SyscallNumber;
    /// 读取 robust futex 列表（`get_robust_list(2)`）。
    const GET_ROBUST_LIST: SyscallNumber;

    // 其它常用（早期阶段优先级不高，但常见）/ other common syscalls
    /// 从内核熵池取随机字节（`getrandom(2)`）。
    const GETRANDOM: SyscallNumber;
    /// 设置间隔定时器（`setitimer(2)`）。
    const SETITIMER: SyscallNumber;
    /// 查询间隔定时器（`getitimer(2)`）。
    const GETITIMER: SyscallNumber;
    /// 查询资源软/硬上限（`getrlimit(2)`）。
    const GETRLIMIT: SyscallNumber;
    /// 查询进程/线程资源使用统计（`getrusage(2)`）。
    const GETRUSAGE: SyscallNumber;
    /// 设置资源软/硬上限（`setrlimit(2)`）。
    const SETRLIMIT: SyscallNumber;
    /// 设置文件创建权限掩码（`umask(2)`）。
    const UMASK: SyscallNumber;
    /// 查询或设置指定进程资源限制（`prlimit64(2)`）。
    const PRLIMIT64: SyscallNumber;
    /// 可中断的纳秒级睡眠（`nanosleep(2)`）。
    const NANOSLEEP: SyscallNumber;
    /// 内核日志环（`syslog(2)` / `sys_syslog`）。
    const SYSLOG: SyscallNumber;

    // Socket / 网络 / socket & networking
    /// 创建 socket（`socket(2)`）。
    const SOCKET: SyscallNumber;
    /// 绑定地址到 socket（`bind(2)`）。
    const BIND: SyscallNumber;
    /// 开始监听（`listen(2)`）。
    const LISTEN: SyscallNumber;
    /// 接受连接（`accept(2)`）。
    const ACCEPT: SyscallNumber;
    /// 接受连接（`accept4(2)`）。
    const ACCEPT4: SyscallNumber;
    /// 发起连接（`connect(2)`）。
    const CONNECT: SyscallNumber;
    /// 获取本地地址（`getsockname(2)`）。
    const GETSOCKNAME: SyscallNumber;
    /// 获取远端地址（`getpeername(2)`）。
    const GETPEERNAME: SyscallNumber;
    /// 发送数据报到指定地址（`sendto(2)`）。
    const SENDTO: SyscallNumber;
    /// 从指定地址接收数据报（`recvfrom(2)`）。
    const RECVFROM: SyscallNumber;
    /// 发送消息（`sendmsg(2)`）。
    const SENDMSG: SyscallNumber;
    /// 接收消息（`recvmsg(2)`）。
    const RECVMSG: SyscallNumber;
    /// 设置 socket 选项（`setsockopt(2)`）。
    const SETSOCKOPT: SyscallNumber;
    /// 获取 socket 选项（`getsockopt(2)`）。
    const GETSOCKOPT: SyscallNumber;
    /// 关闭 socket 发送/接收（`shutdown(2)`）。
    const SHUTDOWN: SyscallNumber;
    /// I/O 多路复用（`ppoll(2)`）。
    const PPOLL: SyscallNumber;
    /// `pselect6(2)`。
    const PSELECT6: SyscallNumber;
    /// `select(2)`。
    const SELECT: SyscallNumber;
    /// 历史/扩展 `poll(2)` 号（WaterOS 曾用 271）。
    const POLL: SyscallNumber;
}
