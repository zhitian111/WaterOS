//! 向内核网络工作任务通知 socket 状态变化，不依赖具体调度器。

use core::sync::atomic::{AtomicUsize, Ordering};

static NOTIFY : AtomicUsize = AtomicUsize::new(0);

/// 安装非阻塞通知函数。初始化时调用；函数不得再次访问协议栈。
pub fn set_activity_notifier(notify : fn()) { NOTIFY.store(notify as usize, Ordering::Release); }

pub(super) fn notify_activity() {
    let address = NOTIFY.load(Ordering::Acquire);
    if address != 0 {
        // SAFETY: NOTIFY 只存储 set_activity_notifier 收到的静态 fn() 指针。
        let notify : fn() = unsafe { core::mem::transmute(address) };
        notify();
    }
}
