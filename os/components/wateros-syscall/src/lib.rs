#![no_std]
//! 用户态系统调用分发：将 ABI 规定的调用号与寄存器参数映射到内核任务、控制台与最小内存桩。
//!
//! **契约**：[`dispatch_syscall_from_trap`] 为 Rust 侧 trap 组合入口；[`__wateros_syscall_dispatch_current`] 供 C ABI（如 `switch` 桩）使用。返回值遵循 `UserRet`/`ErrNo` 编码。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::syscall_number::{ActiveSyscallNumberTable, SyscallNumberTable};
use abi::user_ret::UserRet;
use core::sync::atomic::{AtomicUsize, Ordering};

const SYSCALL_YIELD_NR: usize = <ActiveSyscallNumberTable as SyscallNumberTable>::YIELD.raw();
const SYSCALL_EXIT_NR: usize = <ActiveSyscallNumberTable as SyscallNumberTable>::EXIT.raw();
const SYSCALL_EXIT_GROUP_NR: usize =
    <ActiveSyscallNumberTable as SyscallNumberTable>::EXIT_GROUP.raw();
const SYSCALL_WRITE_NR: usize = <ActiveSyscallNumberTable as SyscallNumberTable>::WRITE.raw();
const SYSCALL_BRK_NR: usize = <ActiveSyscallNumberTable as SyscallNumberTable>::BRK.raw();

/// 用户态 `brk` 的单调递增假顶：满足 `brk(0)` 查询与简单扩展语义，避免返回 0 导致 libc 忙等。
///
/// **当前行为**：首次 `brk(0)` 返回常量初值；`brk(addr)` 仅接受不小于当前顶的地址。**后续替换点**：对接真实 `mmap`/VMA 管理后应删除此原子桩。
static USER_BRK_FAKE: AtomicUsize = AtomicUsize::new(0);

#[inline]
fn dispatch_write(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    if fd != 1 && fd != 2 {
        return UserRet::from_error(ErrNo::EBADF);
    }
    let ptr = args.arg(1) as *const u8;
    let len = args.arg(2);
    if len == 0 {
        return UserRet::from_success(0);
    }
    if len > 4 * 1024 * 1024 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let buf = unsafe { core::slice::from_raw_parts(ptr, len) };
    console::write_raw_bytes(buf);
    UserRet::from_success(len)
}

#[inline]
fn dispatch_brk(addr: usize) -> UserRet {
    // 须高于静态链接用户镜像末端（含大 `.bss` 堆）；仅作 `brk(0)` 查询桩。
    const INITIAL: usize = 0x0120_0000;
    if addr == 0 {
        let v = USER_BRK_FAKE.load(Ordering::Relaxed);
        if v == 0 {
            USER_BRK_FAKE.store(INITIAL, Ordering::Relaxed);
            return UserRet::from_success(INITIAL);
        }
        return UserRet::from_success(v);
    }
    let cur = USER_BRK_FAKE.load(Ordering::Relaxed).max(INITIAL);
    if addr < cur {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    USER_BRK_FAKE.store(addr, Ordering::Relaxed);
    UserRet::from_success(addr)
}

/// Trap / 异常返回路径上应调用的系统调用分发（具名 Rust API；组合层 `trap_handler` 直接调用本函数）。
#[inline]
pub fn dispatch_syscall_from_trap(syscall_nr: usize, syscall_args: SyscallArgs) -> isize {
    match syscall_nr {
        SYSCALL_YIELD_NR => {
            task::yield_now();
            UserRet::from_success(0).0
        }
        SYSCALL_EXIT_NR | SYSCALL_EXIT_GROUP_NR => {
            let exit_code = syscall_args.arg(0) as isize;
            task::exit_current(exit_code)
        }
        SYSCALL_WRITE_NR => dispatch_write(syscall_args).0,
        SYSCALL_BRK_NR => dispatch_brk(syscall_args.arg(0)).0,
        _ => UserRet::from_error(ErrNo::ENOSYS).0,
    }
}

/// 当前任务上的系统调用分发入口：按 `ActiveSyscallNumberTable` 解析 `syscall_nr`，参数来自通用寄存器约定。
///
/// 已识别调用：`YIELD`、`EXIT`/`EXIT_GROUP`、`WRITE`（仅 fd 1/2 走控制台）、`BRK`（见 [`USER_BRK_FAKE`]）；其余返回 `ENOSYS`。
#[unsafe(no_mangle)]
pub extern "C" fn __wateros_syscall_dispatch_current(
    syscall_nr: usize,
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    arg5: usize,
) -> isize {
    let syscall_args = SyscallArgs::from_regs([arg0, arg1, arg2, arg3, arg4, arg5]);
    dispatch_syscall_from_trap(syscall_nr, syscall_args)
}
