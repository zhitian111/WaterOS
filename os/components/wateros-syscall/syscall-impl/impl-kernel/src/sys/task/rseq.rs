//! 可重启序列（rseq）的兼容性占位实现。

use core::sync::atomic::{AtomicBool, Ordering};

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;

static RSEQ_FALLBACK_REPORTED: AtomicBool = AtomicBool::new(false);

/// 在上下文切换和任务迁移钩子能够维护用户态 ABI 前保持禁用，避免暴露不完整语义。
pub(crate) fn sys_rseq(_args: SyscallArgs) -> UserRet {
    if !RSEQ_FALLBACK_REPORTED.swap(true, Ordering::Relaxed) {
        log::trace!("[syscall] rseq unavailable; userspace fallback requested");
    }
    UserRet::from_error(ErrNo::ENOSYS)
}
