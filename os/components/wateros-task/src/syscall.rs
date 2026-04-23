use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

const SYSCALL_YIELD_NR: usize = 124;
const SYSCALL_EXIT_NR: usize = 93;
const SYSCALL_EXIT_GROUP_NR: usize = 94;

/// Dispatch one user syscall into task-runtime semantics.
///
/// The arch trap layer is responsible for trap decoding and register shuffling;
/// this layer owns the mapping from syscall numbers to task/runtime behavior.
#[inline]
pub fn dispatch_current_syscall(syscall_nr: usize, syscall_args: SyscallArgs) -> isize {
    match syscall_nr {
        SYSCALL_YIELD_NR => {
            crate::yield_now();
            UserRet::from_success(0).0
        }
        SYSCALL_EXIT_NR | SYSCALL_EXIT_GROUP_NR => {
            let exit_code = syscall_args.arg(0) as isize;
            crate::exit_current(exit_code)
        }
        _ => UserRet::from_error(ErrNo::ENOSYS).0,
    }
}
