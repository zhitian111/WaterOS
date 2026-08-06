//! 跟踪 `vfork(2)` 子任务的完成状态，并负责挂起和唤醒父任务。

extern crate alloc;

use alloc::collections::BTreeMap;
use spin::Mutex;

use task::{TaskId, WaitQueue};

static VFORK_CHILDREN: Mutex<BTreeMap<TaskId, WaitQueue>> = Mutex::new(BTreeMap::new());

/// 在子任务进入可运行状态前登记它，并返回供父任务等待的队列。
pub(super) fn register(child: TaskId) -> WaitQueue {
    let wait = WaitQueue::new_named("vfork-parent");
    let replaced = VFORK_CHILDREN.lock().insert(child, wait);
    debug_assert!(replaced.is_none());
    wait
}

/// 挂起 vfork 父任务，直到子任务完成 exec 或退出。
pub(super) fn wait_for_completion(child: TaskId, wait: WaitQueue) {
    while VFORK_CHILDREN.lock().contains_key(&child) {
        let _ = wait.wait_current_while(|| VFORK_CHILDREN.lock().contains_key(&child));
    }
    let _ = wait.try_release_empty();
}

/// 判断当前任务是否仍在使用 vfork 父任务的地址空间。
pub(super) fn current_is_child() -> bool {
    task::current_task_id()
        .is_some_and(|task_id| VFORK_CHILDREN.lock().contains_key(&task_id))
}

/// 当前子任务完成 exec 或退出后，解除登记并唤醒被挂起的父任务。
pub(super) fn complete_current() -> bool {
    let Some(task_id) = task::current_task_id() else {
        return false;
    };
    let wait = VFORK_CHILDREN.lock().remove(&task_id);
    if let Some(wait) = wait {
        wait.wake_all();
        true
    } else {
        false
    }
}
