//! 内核 futex 表契约。

use crate::error::{FutexError, FutexResult};
use crate::key::FutexKey;

/// 内核 futex 等待/唤醒表契约。
///
/// 具体实现负责队列表、与用户内存的交互，以及（后续）robust futex 退出清理。
/// syscall 层应通过本 trait 调用，而非直接操作等待队列。
pub trait KernelFutexOps: Sized {
    /// 在 `key` 对应队列上等待，直到用户地址上的 32 位值不再等于 `expected`。
    ///
    /// 调用方须在实现内部或外层保证「检查→睡眠」窗口的原子性。
    fn wait(&self, key : FutexKey, expected : u32) -> FutexResult<()>;

    /// 唤醒 `key` 对应队列上最多 `max_wake` 个等待者；`max_wake == 0` 时语义由实现定义。
    fn wake(&self, key : FutexKey, max_wake : u32) -> FutexResult<usize>;

    /// 唤醒 `key` 对应队列上的全部等待者。
    fn wake_all(&self, key : FutexKey) -> FutexResult<usize>;

    /// 线程异常退出时对 robust futex 链表的清理钩子（占位，完整语义待设计）。
    fn on_thread_exit_robust(&self, _list_head : usize) -> FutexResult<()> {
        Err(FutexError::Nosys)
    }
}
