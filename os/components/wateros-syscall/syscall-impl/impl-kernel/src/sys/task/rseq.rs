//! Restartable-sequences compatibility fallback.

use core::sync::atomic::{AtomicBool, Ordering};

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;

static RSEQ_FALLBACK_REPORTED: AtomicBool = AtomicBool::new(false);

/// Keep rseq disabled until context-switch and migration hooks can maintain its userspace ABI.
pub(crate) fn sys_rseq(_args: SyscallArgs) -> UserRet {
    if !RSEQ_FALLBACK_REPORTED.swap(true, Ordering::Relaxed) {
        log::trace!("[syscall] rseq unavailable; userspace fallback requested");
    }
    UserRet::from_error(ErrNo::ENOSYS)
}
