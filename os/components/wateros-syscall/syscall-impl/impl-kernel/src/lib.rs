#![no_std]
//! Kernel syscall implementation.
//!
//! This crate owns the concrete `sys_*` behavior and implements the dispatcher
//! trait declared by `wateros-syscall-api-v0`.

extern crate alloc;

use abi::syscall_args::SyscallArgs;
use abi::syscall_number::ActiveSyscallNumberTable;

mod linux_stat;
mod mm_util;
mod sys;
mod unsupported;
mod user_copy;
mod vfs_util;

/// Trap / exception return path syscall dispatch entry.
#[inline]
pub fn dispatch_syscall_from_trap(syscall_nr : usize, syscall_args : SyscallArgs) -> isize {
    <KernelSyscallDispatcher as api_v0::SyscallDispatcher>::dispatch_syscall_from_trap(syscall_nr,
                                                                                       syscall_args)
}

/// Kernel syscall implementation selected by the aggregate crate.
pub struct KernelSyscallDispatcher;

impl api_v0::SyscallDispatcher for KernelSyscallDispatcher {
    type NumberTable = ActiveSyscallNumberTable;

    #[inline]
    fn dispatch_yield(_args : SyscallArgs) -> isize { sys::sys_yield().0 }

    #[inline]
    fn dispatch_clone(args : SyscallArgs) -> isize { sys::sys_clone(args).0 }

    #[inline]
    fn dispatch_exit(args : SyscallArgs) -> isize { sys::sys_exit(args.arg(0) as isize) }

    #[inline]
    fn dispatch_read(args : SyscallArgs) -> isize { sys::sys_read(args).0 }

    #[inline]
    fn dispatch_write(args : SyscallArgs) -> isize { sys::sys_write(args).0 }

    #[inline]
    fn dispatch_openat(args : SyscallArgs) -> isize { sys::sys_openat(args).0 }

    #[inline]
    fn dispatch_close(args : SyscallArgs) -> isize { sys::sys_close(args).0 }

    #[inline]
    fn dispatch_fstat(args : SyscallArgs) -> isize { sys::sys_fstat(args).0 }

    #[inline]
    fn dispatch_lseek(args : SyscallArgs) -> isize { sys::sys_lseek(args).0 }

    #[inline]
    fn dispatch_dup(_args : SyscallArgs) -> isize { sys::sys_dup(_args).0 }

    #[inline]
    fn dispatch_dup3(_args : SyscallArgs) -> isize { sys::sys_dup3(_args).0 }

    #[inline]
    fn dispatch_pipe2(args : SyscallArgs) -> isize { sys::sys_pipe2(args).0 }

    #[inline]
    fn dispatch_brk(args : SyscallArgs) -> isize { sys::sys_brk(args.arg(0)).0 }

    #[inline]
    fn dispatch_mmap(args : SyscallArgs) -> isize { sys::sys_mmap(args).0 }

    #[inline]
    fn dispatch_munmap(args : SyscallArgs) -> isize { sys::sys_munmap(args).0 }

    #[inline]
    fn dispatch_mprotect(args : SyscallArgs) -> isize { sys::sys_mprotect(args).0 }

    #[inline]
    fn dispatch_get_time(args : SyscallArgs) -> isize { sys::sys_gettimeofday(args).0 }

    #[inline]
    fn dispatch_clock_gettime(args : SyscallArgs) -> isize { sys::sys_clock_gettime(args).0 }

    #[inline]
    fn dispatch_getpid(_args : SyscallArgs) -> isize { sys::sys_getpid().0 }

    #[inline]
    fn dispatch_getppid(_args : SyscallArgs) -> isize { sys::sys_getppid().0 }

    #[inline]
    fn dispatch_gettid(_args : SyscallArgs) -> isize { sys::sys_gettid().0 }

    #[inline]
    fn dispatch_waitpid(args : SyscallArgs) -> isize { sys::sys_waitpid(args).0 }

    #[inline]
    fn dispatch_nanosleep(args : SyscallArgs) -> isize { sys::sys_nanosleep(args).0 }

    #[inline]
    fn dispatch_times(args : SyscallArgs) -> isize { sys::sys_times(args).0 }

    #[inline]
    fn dispatch_getcwd(args : SyscallArgs) -> isize { sys::sys_getcwd(args).0 }

    #[inline]
    fn dispatch_chdir(args : SyscallArgs) -> isize { sys::sys_chdir(args).0 }

    #[inline]
    fn dispatch_uname(args : SyscallArgs) -> isize { sys::sys_uname(args).0 }

    #[inline]
    fn dispatch_prctl(args : SyscallArgs) -> isize { sys::sys_prctl(args).0 }

    #[inline]
    fn dispatch_getrlimit(args : SyscallArgs) -> isize { sys::sys_getrlimit(args).0 }

    #[inline]
    fn dispatch_setrlimit(args : SyscallArgs) -> isize { sys::sys_setrlimit(args).0 }

    #[inline]
    fn dispatch_set_tid_address(_args : SyscallArgs) -> isize { sys::sys_set_tid_address().0 }

    #[inline]
    fn dispatch_unknown(syscall_nr : usize, args : SyscallArgs) -> isize {
        unsupported::syscall_unknown(syscall_nr, args);
    }
}
