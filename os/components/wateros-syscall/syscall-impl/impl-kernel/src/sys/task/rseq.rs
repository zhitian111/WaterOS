//! Restartable-sequences compatibility fallback.

use core::sync::atomic::{AtomicBool, Ordering};

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

static RSEQ_FALLBACK_REPORTED: AtomicBool = AtomicBool::new(false);

/// Keep rseq disabled until context-switch and migration hooks can maintain its userspace ABI.
pub(crate) fn sys_rseq(_args: SyscallArgs) -> UserRet {
    if !RSEQ_FALLBACK_REPORTED.swap(true, Ordering::Relaxed) {
        log::trace!("[syscall] rseq unavailable; userspace fallback requested");
    }
    UserRet::from_error(ErrNo::ENOSYS)
}
