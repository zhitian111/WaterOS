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
mod socket_fd;
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

const SYS_STATX : usize = 291;

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
    fn dispatch_getuid(_args : SyscallArgs) -> isize { sys::sys_getuid().0 }

    #[inline]
    fn dispatch_geteuid(_args : SyscallArgs) -> isize { sys::sys_geteuid().0 }

    #[inline]
    fn dispatch_getgid(_args : SyscallArgs) -> isize { sys::sys_getgid().0 }

    #[inline]
    fn dispatch_getegid(_args : SyscallArgs) -> isize { sys::sys_getegid().0 }

    #[inline]
    fn dispatch_getgroups(args : SyscallArgs) -> isize { sys::sys_getgroups(args).0 }

    #[inline]
    fn dispatch_setuid(args : SyscallArgs) -> isize { sys::sys_setuid(args) }

    #[inline]
    fn dispatch_setgid(args : SyscallArgs) -> isize { sys::sys_setgid(args) }

    #[inline]
    fn dispatch_setreuid(args : SyscallArgs) -> isize { sys::sys_setreuid(args) }

    #[inline]
    fn dispatch_setregid(args : SyscallArgs) -> isize { sys::sys_setregid(args) }

    #[inline]
    fn dispatch_setresuid(args : SyscallArgs) -> isize { sys::sys_setresuid(args) }

    #[inline]
    fn dispatch_setresgid(args : SyscallArgs) -> isize { sys::sys_setresgid(args) }

    #[inline]
    fn dispatch_futex(args : SyscallArgs) -> isize { sys::sys_futex(args).0 }

    #[inline]
    fn dispatch_fcntl(args : SyscallArgs) -> isize { sys::sys_fcntl(args).0 }

    #[inline]
    fn dispatch_execve(args : SyscallArgs) -> isize { sys::sys_execve(args).0 }

    #[inline]
    fn dispatch_waitpid(args : SyscallArgs) -> isize { sys::sys_waitpid(args).0 }

    #[inline]
    fn dispatch_kill(args : SyscallArgs) -> isize { sys::sys_kill(args).0 }

    #[inline]
    fn dispatch_nanosleep(args : SyscallArgs) -> isize { sys::sys_nanosleep(args).0 }

    #[inline]
    fn dispatch_times(args : SyscallArgs) -> isize { sys::sys_times(args).0 }

    #[inline]
    fn dispatch_getcwd(args : SyscallArgs) -> isize { sys::sys_getcwd(args).0 }

    #[inline]
    fn dispatch_chdir(args : SyscallArgs) -> isize { sys::sys_chdir(args).0 }

    #[inline]
    fn dispatch_mkdirat(args : SyscallArgs) -> isize { sys::sys_mkdirat(args).0 }

    #[inline]
    fn dispatch_getdents64(args : SyscallArgs) -> isize { sys::sys_getdents64(args).0 }

    #[inline]
    fn dispatch_unlinkat(args : SyscallArgs) -> isize { sys::sys_unlinkat(args).0 }

    #[inline]
    fn dispatch_mount(args : SyscallArgs) -> isize { sys::sys_mount(args).0 }

    #[inline]
    fn dispatch_umount2(args : SyscallArgs) -> isize { sys::sys_umount2(args).0 }

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
        if syscall_nr == SYS_STATX {
            return sys::sys_statx(args).0;
        }
        unsupported::syscall_unknown(syscall_nr, args);
    }

    // ——— socket / 网络 ———

    #[inline]
    fn dispatch_socket(args: SyscallArgs) -> isize { sys::sys_socket(args).0 }

    #[inline]
    fn dispatch_bind(args: SyscallArgs) -> isize { sys::sys_bind(args).0 }

    #[inline]
    fn dispatch_listen(args: SyscallArgs) -> isize { sys::sys_listen(args).0 }

    #[inline]
    fn dispatch_accept4(args: SyscallArgs) -> isize { sys::sys_accept4(args).0 }

    #[inline]
    fn dispatch_connect(args: SyscallArgs) -> isize { sys::sys_connect(args).0 }

    #[inline]
    fn dispatch_getsockname(args: SyscallArgs) -> isize { sys::sys_getsockname(args).0 }

    #[inline]
    fn dispatch_getpeername(args: SyscallArgs) -> isize { sys::sys_getpeername(args).0 }

    #[inline]
    fn dispatch_sendto(args: SyscallArgs) -> isize { sys::sys_sendto(args).0 }

    #[inline]
    fn dispatch_recvfrom(args: SyscallArgs) -> isize { sys::sys_recvfrom(args).0 }

    #[inline]
    fn dispatch_sendmsg(args: SyscallArgs) -> isize { sys::sys_sendmsg(args).0 }

    #[inline]
    fn dispatch_recvmsg(args: SyscallArgs) -> isize { sys::sys_recvmsg(args).0 }

    #[inline]
    fn dispatch_setsockopt(args: SyscallArgs) -> isize { sys::sys_setsockopt(args).0 }

    #[inline]
    fn dispatch_getsockopt(args: SyscallArgs) -> isize { sys::sys_getsockopt(args).0 }

    #[inline]
    fn dispatch_shutdown(args: SyscallArgs) -> isize { sys::sys_shutdown(args).0 }

    #[inline]
    fn dispatch_poll(args: SyscallArgs) -> isize { sys::sys_poll(args).0 }
}
