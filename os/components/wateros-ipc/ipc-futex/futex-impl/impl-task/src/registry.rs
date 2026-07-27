//! Futex 等待队列与 robust 状态表。

use alloc::collections::BTreeMap;

use api_v0::FutexKey;
use ipc_waitqueue::WaitQueue;
use task_api::TaskId;

struct FutexQueue {
    wait_queue : WaitQueue,
    /// 已取得此队列、但尚未完成锁外 scheduler 操作的使用者数量。
    ///
    /// 非零时不能释放 `WaitQueueId`，否则并发 wait/wake 可能操作复用后的 ID。
    active_users : usize,
}

#[derive(Clone, Copy)]
struct RobustState {
    head : usize,
    len : usize,
}

pub(crate) struct FutexRegistry {
    queues : BTreeMap<FutexKey, FutexQueue>,
    robust : BTreeMap<TaskId, RobustState>,
}

impl FutexRegistry {
    pub const fn new() -> Self {
        Self { queues : BTreeMap::new(),
               robust : BTreeMap::new() }
    }

    pub fn acquire_queue(&mut self, key : FutexKey) -> WaitQueue {
        let queue = self.queues
                        .entry(key)
                        .or_insert_with(|| FutexQueue { wait_queue : WaitQueue::new(),
                                                        active_users : 0 });
        queue.active_users = queue.active_users
                                  .saturating_add(1);
        queue.wait_queue
    }

    pub fn acquire_existing_queue(&mut self, key : FutexKey) -> Option<WaitQueue> {
        let queue = self.queues
                        .get_mut(&key)?;
        queue.active_users = queue.active_users
                                  .saturating_add(1);
        Some(queue.wait_queue)
    }

    pub fn acquire_requeue_queues(&mut self,
                                  from_key : FutexKey,
                                  to_key : FutexKey)
                                  -> Option<(WaitQueue, WaitQueue)> {
        let from_queue = self.acquire_existing_queue(from_key)?;
        let to_queue = self.acquire_queue(to_key);
        Some((from_queue, to_queue))
    }

    pub fn release_queue(&mut self, key : FutexKey) {
        if let Some(queue) = self.queues
                                 .get_mut(&key)
        {
            queue.active_users = queue.active_users
                                      .saturating_sub(1);
        }
        self.cleanup_empty_queue(key);
    }

    fn cleanup_empty_queue(&mut self, key : FutexKey) {
        let Some(queue) = self.queues
                              .get(&key)
        else {
            return;
        };
        if queue.active_users == 0 &&
           queue.wait_queue
                .try_release_empty()
        {
            self.queues
                .remove(&key);
        }
    }

    pub fn set_robust_list(&mut self, task_id : TaskId, head : usize, len : usize) {
        self.robust
            .insert(task_id, RobustState { head, len });
    }

    pub fn robust_list(&self, task_id : TaskId) -> (usize, usize) {
        self.robust
            .get(&task_id)
            .map(|state| (state.head, state.len))
            .unwrap_or((0, 0))
    }

    pub fn take_robust_list(&mut self, task_id : TaskId) -> (usize, usize) {
        self.robust
            .remove(&task_id)
            .map(|state| (state.head, state.len))
            .unwrap_or((0, 0))
    }

    pub fn drop_robust_list(&mut self, task_id : TaskId) {
        self.robust
            .remove(&task_id);
    }
}
