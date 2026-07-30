//! Futex 等待队列与 robust 状态表。
//!
//! ARCH: 本文件只管理 key 到 `WaitQueue` 的生命周期及 per-task 侧表；实际任务
//! 状态切换在 `ipc-waitqueue` / `wateros-task` 中完成。

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::AtomicU64;

use api_v0::{FutexKey, FutexWaitOutcome, RobustListRegistration, ROBUST_LIST_HEAD_SIZE};
use ipc_waitqueue::WaitQueue;
use task_api::TaskId;

/// DATA: 一个已被至少一个 waiter 或并发操作引用的 futex key。
///
/// 该结构始终由 `FutexRegistry::queues` 持有，且只能在 registry 锁内创建、
/// 递增/递减 `active_users` 或删除。
struct FutexQueue {
    wait_queue : WaitQueue,
    /// 先于 scheduler wake 递增；等待者在 scheduler 临界区只需比较此序列，
    /// 无需在持有 scheduler 锁时访问用户地址空间。
    wake_sequence : Arc<AtomicU64>,
    /// 已取得此队列、但尚未完成锁外 scheduler 操作的使用者数量。
    ///
    /// 非零时不能释放 `WaitQueueId`，否则并发 wait/wake 可能操作复用后的 ID。
    active_users : usize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FutexQueueDebug {
    pub wait_queue_id : usize,
    /// 包含睡眠 waiter 和尚未结束的并发 wait/wake 操作者。
    pub active_users : usize,
}

pub(crate) struct FutexDebugSnapshot {
    pub wait_attempts : u64,
    pub wait_returns : u64,
    pub wake_calls : u64,
    pub woken_tasks : u64,
    pub requeue_calls : u64,
    pub last_wait : Option<(FutexKey, usize)>,
    pub last_wait_result : Option<(FutexKey, FutexWaitOutcome)>,
    pub last_wake : Option<(FutexKey, u32, usize)>,
    pub last_requeue : Option<(FutexKey, FutexKey, u32, u32, usize)>,
    pub queues : Vec<FutexQueueDebug>,
}

/// DATA: futex 模块的全部可变元数据。
///
/// LOCK: 所有字段由 `global.rs` 的 `REGISTRY` 独占保护。任何从本结构返回的
/// `WaitQueue` 都可在解锁后使用，但调用者必须以 `release_queue` 对称归还其
/// `active_users` 使用权。
pub(crate) struct FutexRegistry {
    /// 每个正在使用的 futex key 对应的 scheduler 等待队列。
    queues : BTreeMap<FutexKey, FutexQueue>,
    /// DATA: 一个 task 同时只能挂在一个 futex key 上；用于异常退出收尾。
    waiting_tasks : BTreeMap<TaskId, FutexKey>,
    /// DATA: task 到用户 robust 链表登记的侧表；不保存或解引用用户内存。
    robust : BTreeMap<TaskId, RobustListRegistration>,
    wait_attempts : u64,
    wait_returns : u64,
    wake_calls : u64,
    woken_tasks : u64,
    requeue_calls : u64,
    last_wait : Option<(FutexKey, usize)>,
    last_wait_result : Option<(FutexKey, FutexWaitOutcome)>,
    last_wake : Option<(FutexKey, u32, usize)>,
    last_requeue : Option<(FutexKey, FutexKey, u32, u32, usize)>,
}

impl FutexRegistry {
    pub const fn new() -> Self {
        Self { queues : BTreeMap::new(),
               waiting_tasks : BTreeMap::new(),
               robust : BTreeMap::new(),
               wait_attempts : 0,
               wait_returns : 0,
               wake_calls : 0,
               woken_tasks : 0,
               requeue_calls : 0,
               last_wait : None,
               last_wait_result : None,
               last_wake : None,
               last_requeue : None }
    }

    /// 取得或创建 key 的队列，并为即将发生的锁外操作增加使用权。
    ///
    /// INVARIANT: 成功调用一次，调用方最终必须调用一次 [`Self::release_queue`]。
    pub fn acquire_queue(&mut self, key : FutexKey) -> (WaitQueue, Arc<AtomicU64>) {
        let queue = self.queues
                        .entry(key)
                        .or_insert_with(|| FutexQueue { wait_queue : WaitQueue::new_named("futex"),
                                                        wake_sequence : Arc::new(AtomicU64::new(0)),
                                                        active_users : 0 });
        queue.active_users = queue.active_users
                                  .saturating_add(1);
        (queue.wait_queue,
         queue.wake_sequence
              .clone())
    }

    /// 取得既有队列；缺失时不创建空队列，供 wake 快速返回 0。
    pub fn acquire_existing_queue(&mut self,
                                  key : FutexKey)
                                  -> Option<(WaitQueue, Arc<AtomicU64>)> {
        let queue = self.queues
                        .get_mut(&key)?;
        queue.active_users = queue.active_users
                                  .saturating_add(1);
        Some((queue.wait_queue,
              queue.wake_sequence
                   .clone()))
    }

    /// 同时取得 requeue 的源/目标队列。
    ///
    /// 源队列必须已存在；目标队列可按需创建。成功时两个队列都各有一次需要
    /// 归还的使用权。
    pub fn acquire_requeue_queues(
        &mut self,
        from_key : FutexKey,
        to_key : FutexKey)
        -> Option<((WaitQueue, Arc<AtomicU64>), (WaitQueue, Arc<AtomicU64>))> {
        let from_queue = self.acquire_existing_queue(from_key)?;
        let to_queue = self.acquire_queue(to_key);
        Some((from_queue, to_queue))
    }

    /// 归还一次锁外操作使用权，并在队列确实为空时释放其 `WaitQueueId`。
    pub fn release_queue(&mut self, key : FutexKey) {
        if let Some(queue) = self.queues
                                 .get_mut(&key)
        {
            queue.active_users = queue.active_users
                                      .saturating_sub(1);
        }
        self.cleanup_empty_queue(key);
    }

    /// 发布 task 正在等待 key 的登记。
    ///
    /// INVARIANT: 若 task 已登记在其他 key，旧 key 的使用权必须立即归还，
    /// 从而维持一个 task 至多对应一个 futex wait 的关系。
    pub fn register_waiting_task(&mut self, task_id : TaskId, key : FutexKey) {
        let previous = self.waiting_tasks
                           .insert(task_id, key);
        if let Some(previous_key) = previous {
            self.release_queue(previous_key);
        }
    }

    pub fn finish_waiting_task(&mut self, task_id : TaskId, key : FutexKey) {
        if self.waiting_tasks
               .get(&task_id) ==
           Some(&key)
        {
            self.waiting_tasks
                .remove(&task_id);
            self.release_queue(key);
        }
    }

    pub fn cancel_waiting_task(&mut self, task_id : TaskId) {
        if let Some(key) = self.waiting_tasks
                               .remove(&task_id)
        {
            self.release_queue(key);
        }
    }

    pub fn record_wait_attempt(&mut self, key : FutexKey, wait_queue_id : usize) {
        self.wait_attempts = self.wait_attempts
                                 .saturating_add(1);
        self.last_wait = Some((key, wait_queue_id));
    }

    pub fn record_wait_result(&mut self, key : FutexKey, outcome : FutexWaitOutcome) {
        self.wait_returns = self.wait_returns
                                .saturating_add(1);
        self.last_wait_result = Some((key, outcome));
    }

    pub fn record_wake(&mut self, key : FutexKey, requested : u32, woken : usize) {
        self.wake_calls = self.wake_calls
                              .saturating_add(1);
        self.woken_tasks = self.woken_tasks
                               .saturating_add(woken as u64);
        self.last_wake = Some((key, requested, woken));
    }

    pub fn record_requeue(&mut self,
                          from_key : FutexKey,
                          to_key : FutexKey,
                          wake_count : u32,
                          requeue_count : u32,
                          changed : usize) {
        self.requeue_calls = self.requeue_calls
                                 .saturating_add(1);
        self.last_requeue = Some((from_key, to_key, wake_count, requeue_count, changed));
    }

    pub fn debug_snapshot(&self) -> FutexDebugSnapshot {
        let queues = self.queues
                         .iter()
                         .map(|(_, queue)| FutexQueueDebug { wait_queue_id : queue.wait_queue
                                                                                  .id(),
                                                             active_users : queue.active_users })
                         .collect();
        FutexDebugSnapshot { wait_attempts : self.wait_attempts,
                             wait_returns : self.wait_returns,
                             wake_calls : self.wake_calls,
                             woken_tasks : self.woken_tasks,
                             requeue_calls : self.requeue_calls,
                             last_wait : self.last_wait,
                             last_wait_result : self.last_wait_result,
                             last_wake : self.last_wake,
                             last_requeue : self.last_requeue,
                             queues }
    }

    /// 尝试删除无人使用且没有 scheduler waiter 的队列。
    ///
    /// LOCK: 只能由 registry 锁内的 `release_queue` 调用；`active_users` 是防止
    /// `WaitQueueId` 在锁外 wait/wake/requeue 仍在使用时被复用的关键保护。
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

    pub fn set_robust_list(&mut self,
                           task_id : TaskId,
                           head : usize,
                           len : usize,
                           user_aspace : usize) {
        self.robust
            .insert(task_id, RobustListRegistration { head,
                                                      len,
                                                      user_aspace });
    }

    pub fn robust_list(&self, task_id : TaskId) -> (usize, usize) {
        self.robust
            .get(&task_id)
            .map(|state| (state.head, state.len))
            .unwrap_or((0, ROBUST_LIST_HEAD_SIZE))
    }

    pub fn take_robust_list(&mut self, task_id : TaskId) -> Option<RobustListRegistration> {
        self.robust
            .remove(&task_id)
    }

    pub fn drop_robust_list(&mut self, task_id : TaskId) {
        self.robust
            .remove(&task_id);
    }
}
