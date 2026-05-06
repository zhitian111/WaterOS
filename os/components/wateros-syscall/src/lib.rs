#![no_std]

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::syscall_number::{ActiveSyscallNumberTable, SyscallNumberTable};
use abi::user_ret::UserRet;

const SYSCALL_YIELD_NR: usize = <ActiveSyscallNumberTable as SyscallNumberTable>::YIELD.raw();
const SYSCALL_EXIT_NR: usize = <ActiveSyscallNumberTable as SyscallNumberTable>::EXIT.raw();
const SYSCALL_EXIT_GROUP_NR: usize =
    <ActiveSyscallNumberTable as SyscallNumberTable>::EXIT_GROUP.raw();

#[inline]
fn dispatch_current_syscall(syscall_nr: usize, syscall_args: SyscallArgs) -> isize {
    match syscall_nr {
        SYSCALL_YIELD_NR => {
            task::yield_now();
            UserRet::from_success(0).0
        }
        SYSCALL_EXIT_NR | SYSCALL_EXIT_GROUP_NR => {
            let exit_code = syscall_args.arg(0) as isize;
            task::exit_current(exit_code)
        }
        _ => UserRet::from_error(ErrNo::ENOSYS).0,
    }
}

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
    dispatch_current_syscall(syscall_nr, syscall_args)
}
