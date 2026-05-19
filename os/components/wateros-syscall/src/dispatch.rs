//! 系统调用号路由：将 trap 传入的 `syscall_nr` 分派到 `sys::*` 实现。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::syscall_number::{ActiveSyscallNumberTable, SyscallNumberTable};
use abi::user_ret::UserRet;

use crate::sys;

// 与当前 `ActiveSyscallNumberTable` 一致的调用号常量，供 `match` 与 trap
// 侧传入的 `syscall_nr` 做整数比较（避免运行时查表）。
const SYSCALL_YIELD_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::YIELD.raw();
const SYSCALL_EXIT_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::EXIT.raw();
const SYSCALL_EXIT_GROUP_NR : usize =
    <ActiveSyscallNumberTable as SyscallNumberTable>::EXIT_GROUP.raw();
const SYSCALL_READ_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::READ.raw();
const SYSCALL_WRITE_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::WRITE.raw();
const SYSCALL_CLOSE_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::CLOSE.raw();
const SYSCALL_PIPE2_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::PIPE2.raw();
const SYSCALL_BRK_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::BRK.raw();
const SYSCALL_MMAP_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::MMAP.raw();
const SYSCALL_MUNMAP_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::MUNMAP.raw();
const SYSCALL_MPROTECT_NR : usize =
    <ActiveSyscallNumberTable as SyscallNumberTable>::MPROTECT.raw();
const SYSCALL_WAITPID_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::WAITPID.raw();
const SYSCALL_GET_TIME_NR : usize =
    <ActiveSyscallNumberTable as SyscallNumberTable>::GET_TIME.raw();
const SYSCALL_GETPID_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::GETPID.raw();
const SYSCALL_GETTID_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::GETTID.raw();
const SYSCALL_NANOSLEEP_NR : usize =
    <ActiveSyscallNumberTable as SyscallNumberTable>::NANOSLEEP.raw();

/// Trap / 异常返回路径上应调用的系统调用分发（具名 Rust API；组合层
/// `trap_handler` 直接调用本函数）。
#[inline]
pub fn dispatch_syscall_from_trap(syscall_nr : usize, syscall_args : SyscallArgs) -> isize {
    match syscall_nr {
        SYSCALL_YIELD_NR => sys::sys_yield().0,
        SYSCALL_EXIT_NR | SYSCALL_EXIT_GROUP_NR => {
            sys::sys_exit(syscall_args.arg(0) as isize)
        }
        SYSCALL_READ_NR => sys::sys_read(syscall_args).0,
        SYSCALL_WRITE_NR => sys::sys_write(syscall_args).0,
        SYSCALL_CLOSE_NR => sys::sys_close(syscall_args).0,
        SYSCALL_PIPE2_NR => sys::sys_pipe2(syscall_args).0,
        SYSCALL_BRK_NR => sys::sys_brk(syscall_args.arg(0)).0,
        SYSCALL_MMAP_NR => sys::sys_mmap(syscall_args).0,
        SYSCALL_MUNMAP_NR => sys::sys_munmap(syscall_args).0,
        SYSCALL_MPROTECT_NR => sys::sys_mprotect(syscall_args).0,
        SYSCALL_GET_TIME_NR => sys::sys_get_time().0,
        SYSCALL_GETPID_NR | SYSCALL_GETTID_NR => sys::sys_getpid().0,
        SYSCALL_WAITPID_NR => sys::sys_waitpid(syscall_args).0,
        SYSCALL_NANOSLEEP_NR => sys::sys_nanosleep(syscall_args).0,
        _ => UserRet::from_error(ErrNo::ENOSYS).0,
    }
}

/// 当前任务上的系统调用分发入口：按 `ActiveSyscallNumberTable` 解析
/// `syscall_nr`，参数来自通用寄存器约定。
///
/// **ABI**：`extern "C"` 且 `#[unsafe(no_mangle)]`，符号名固定，供汇编或 C
/// 侧按平台调用约定直接跳转；六个 `usize` 与 `SyscallArgs::from_regs`
/// 所用寄存器槽顺序一致。
#[unsafe(no_mangle)]
pub extern "C" fn __wateros_syscall_dispatch_current(syscall_nr : usize,
                                                     arg0 : usize,
                                                     arg1 : usize,
                                                     arg2 : usize,
                                                     arg3 : usize,
                                                     arg4 : usize,
                                                     arg5 : usize)
                                                     -> isize {
    let syscall_args = SyscallArgs::from_regs([arg0, arg1, arg2, arg3, arg4, arg5]);
    dispatch_syscall_from_trap(syscall_nr, syscall_args)
}
