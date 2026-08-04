//! 全局协议栈实例及其锁访问边界。
//!
//! 只有本模块直接接触全局锁；传输协议模块通过 [`with_stack`] 或
//! [`with_stack_mut`] 在短临界区内调用 [`NetworkStack`] 的方法。

use super::state::NetworkStack;
use super::types::{NetworkError, NetworkResult};

fn debug_cpu_id() -> usize { arch::cpu::current_cpu_id().raw() }

/// 全局协议栈锁是 socket 卡死时最关键的 wait-for 节点。包装类型保留原有
/// `.lock()` API，只有 `gdb-debug` 构建会发布 owner/contention。
static NETWORK_STACK : debug::TrackedMutex<Option<NetworkStack>> =
    debug::TrackedMutex::new(None,
                             debug::DebugLockKind::Network,
                             debug_cpu_id);

/// 安装唯一的协议栈实例。
pub(super) fn install_stack(stack : NetworkStack) -> NetworkResult<()> {
    let mut slot = NETWORK_STACK.lock();
    if slot.is_some() {
        return Err(NetworkError::AlreadyInitialized);
    }
    *slot = Some(stack);
    Ok(())
}

/// 在协议栈锁内执行只读操作。
pub(super) fn with_stack<R, E>(unavailable : E,
                               f : impl FnOnce(&NetworkStack) -> Result<R, E>)
                               -> Result<R, E> {
    let guard = NETWORK_STACK.lock();
    let stack = guard.as_ref()
                     .ok_or(unavailable)?;
    f(stack)
}

/// 在协议栈锁内执行可变操作。
pub(super) fn with_stack_mut<R, E>(unavailable : E,
                                   f : impl FnOnce(&mut NetworkStack) -> Result<R, E>)
                                   -> Result<R, E> {
    let mut guard = NETWORK_STACK.lock();
    let stack = guard.as_mut()
                     .ok_or(unavailable)?;
    f(stack)
}

/// 协议栈尚未初始化时跳过操作，供早期启动和周期性 poll 使用。
pub(super) fn with_stack_if_ready<R>(f : impl FnOnce(&mut NetworkStack) -> R) -> Option<R> {
    let mut guard = NETWORK_STACK.lock();
    guard.as_mut()
         .map(f)
}
