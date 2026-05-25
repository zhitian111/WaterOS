//! `fcntl(2)` — 文件描述符控制。
//!
//! 单线程系统下仅实现 glibc 启动期常用的最小子集。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

// ── fcntl 命令 ────────────────────────────────────────────────────

const F_DUPFD : usize = 0;
const F_GETFD : usize = 1;
const F_SETFD : usize = 2;
const F_GETFL : usize = 3;
const F_SETFL : usize = 4;

// ── 公开入口 ─────────────────────────────────────────────────────

pub(crate) fn sys_fcntl(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let cmd = args.arg(1);
    let arg = args.arg(2);

    let result = match cmd {
        F_DUPFD => fcntl_dupfd(fd, arg),
        F_GETFD => fcntl_getfd(),
        F_SETFD => fcntl_setfd(),
        F_GETFL => fcntl_getfl(fd),
        F_SETFL => fcntl_setfl(fd, arg),
        _ => Err(ErrNo::ENOSYS),
    };

    match result {
        Ok(n) => UserRet::from_success(n),
        Err(e) => UserRet::from_error(e),
    }
}

// ── 各命令实现 ───────────────────────────────────────────────────

/// `F_DUPFD`——复制 fd 到 ≥arg 的最小编号。
fn fcntl_dupfd(_fd : usize, _arg : usize) -> Result<usize, ErrNo> {
    // dup 尚未实现，此处返回 ENOSYS
    Err(ErrNo::ENOSYS)
}

/// `F_GETFD`——返回 fd 自身标志（当前仅 `FD_CLOEXEC`）。
fn fcntl_getfd() -> Result<usize, ErrNo> {
    // 单线程无 exec，FD_CLOEXEC 恒为 0
    Ok(0)
}

/// `F_SETFD`——设置 `FD_CLOEXEC`。
fn fcntl_setfd() -> Result<usize, ErrNo> {
    // 忽略，单线程无影响
    Ok(0)
}

/// `F_GETFL`——返回文件打开时的状态标志（O_RDONLY/O_WRONLY/O_RDWR/O_APPEND
/// 等）。
fn fcntl_getfl(_fd : usize) -> Result<usize, ErrNo> {
    // 暂不跟踪句柄的 open flags，返回 O_RDWR 作为默认值，
    // 避免 glibc 因 O_RDONLY 误判而跳过写操作。
    Ok(0x2) // O_RDWR
}

/// `F_SETFL`——设置文件状态标志（仅允许 O_APPEND、O_NONBLOCK）。
fn fcntl_setfl(_fd : usize, _arg : usize) -> Result<usize, ErrNo> {
    // 忽略，单线程无阻塞 I/O 场景
    Ok(0)
}
