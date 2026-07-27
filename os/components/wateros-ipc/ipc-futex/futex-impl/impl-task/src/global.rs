//! 全局 futex 服务。
//!
//! 公开函数隐藏 registry 与锁。等待和唤醒期间不会长期持有 registry 锁；
//! `active_users` 保护锁外 scheduler 操作所引用的等待队列。

use api_v0::{FutexError, FutexKey, FutexResult, FutexWaitOutcome, ROBUST_LIST_HEAD_SIZE};
use core::sync::atomic::Ordering;
use spin::Mutex;
use task_api::{TaskId, TaskTick, TaskWaitResult};

use crate::registry::FutexRegistry;

/// 唯一的 futex 状态实例；registry 和锁均不跨越本模块边界。
static REGISTRY : Mutex<FutexRegistry> = Mutex::new(FutexRegistry::new());

fn with_registry<R>(f : impl FnOnce(&mut FutexRegistry) -> R) -> R {
    let mut registry = REGISTRY.lock();
    f(&mut registry)
}

/// 在 `key` 对应队列上等待，阻塞前通过 `condition` 再次确认用户态条件。
pub fn wait_while(key : FutexKey,
                  timeout : Option<TaskTick>,
                  mut condition : impl FnMut() -> bool)
                  -> FutexWaitOutcome {
    if !condition() {
        return FutexWaitOutcome::Woken;
    }

    let (wait_queue, wake_sequence) = with_registry(|registry| {
        let (wait_queue, wake_sequence) = registry.acquire_queue(key);
        registry.record_wait_attempt(key, wait_queue.id());
        (wait_queue, wake_sequence)
    });
    let observed_wake = wake_sequence.load(Ordering::Acquire);
    if !condition() {
        with_registry(|registry| {
            registry.record_wait_result(key, FutexWaitOutcome::Woken);
            registry.release_queue(key);
        });
        return FutexWaitOutcome::Woken;
    }

    // condition 已在 scheduler 锁外复查。进入调度临界区后只比较原子 wake
    // 序列：若 wake 发生在“复查—入队”窗口，序列变化会阻止当前任务睡眠；
    // 若 wake 稍后发生，唤醒者会等待 scheduler 锁并看到已入队任务。
    let not_woken = || wake_sequence.load(Ordering::Acquire) == observed_wake;
    let outcome = match timeout {
        None => match wait_queue.wait_current_while(not_woken) {
            TaskWaitResult::Interrupted => FutexWaitOutcome::Interrupted,
            _ => FutexWaitOutcome::Woken,
        },
        Some(0) => {
            if condition() {
                FutexWaitOutcome::TimedOut
            } else {
                FutexWaitOutcome::Woken
            }
        }
        Some(ticks) => match wait_queue.wait_current_while_for_ticks(ticks, not_woken) {
            TaskWaitResult::Woken => FutexWaitOutcome::Woken,
            TaskWaitResult::TimedOut => FutexWaitOutcome::TimedOut,
            TaskWaitResult::Interrupted => FutexWaitOutcome::Interrupted,
        },
    };

    with_registry(|registry| {
        registry.record_wait_result(key, outcome);
        registry.release_queue(key);
    });
    outcome
}

/// 唤醒 `key` 队列上最多 `max_wake` 个等待者。
pub fn wake(key : FutexKey, max_wake : u32) -> usize {
    if max_wake == 0 {
        return 0;
    }
    let Some((wait_queue, wake_sequence)) = with_registry(|registry| {
        let queue = registry.acquire_existing_queue(key);
        if queue.is_none() {
            registry.record_wake(key, max_wake, 0);
        }
        queue
    }) else {
        return 0;
    };

    wake_sequence.fetch_add(1, Ordering::Release);
    let mut woken = 0;
    for _ in 0..max_wake {
        if wait_queue.wake_one()
                     .is_none()
        {
            break;
        }
        woken += 1;
    }
    with_registry(|registry| {
        registry.record_wake(key, max_wake, woken);
        registry.release_queue(key);
    });
    woken
}

/// 唤醒 `key` 队列上的全部等待者。
pub fn wake_all(key : FutexKey) -> usize {
    let Some((wait_queue, wake_sequence)) = with_registry(|registry| {
        let queue = registry.acquire_existing_queue(key);
        if queue.is_none() {
            registry.record_wake(key, u32::MAX, 0);
        }
        queue
    }) else {
        return 0;
    };
    wake_sequence.fetch_add(1, Ordering::Release);
    let woken = wait_queue.wake_all();
    with_registry(|registry| {
        registry.record_wake(key, u32::MAX, woken);
        registry.release_queue(key);
    });
    woken
}

/// 唤醒 `from_key` 上的部分等待者，并把后续等待者迁移到 `to_key`。
pub fn requeue(from_key : FutexKey,
               to_key : FutexKey,
               wake_count : u32,
               requeue_count : u32)
               -> FutexResult<usize> {
    if from_key == to_key {
        return Err(FutexError::Invalid);
    }
    let Some(((from_queue, from_wake_sequence), (to_queue, _to_wake_sequence))) =
        with_registry(|registry| {
        let queues = registry.acquire_requeue_queues(from_key, to_key);
        if queues.is_none() {
            registry.record_requeue(from_key,
                                    to_key,
                                    wake_count,
                                    requeue_count,
                                    0);
        }
        queues
    })
    else {
        return Ok(0);
    };

    from_wake_sequence.fetch_add(1, Ordering::Release);
    let changed = from_queue.requeue_to(to_queue,
                                        wake_count as usize,
                                        requeue_count as usize);
    with_registry(|registry| {
        registry.record_requeue(from_key,
                                to_key,
                                wake_count,
                                requeue_count,
                                changed);
        registry.release_queue(from_key);
        registry.release_queue(to_key);
    });
    Ok(changed)
}

/// 输出 futex registry 的低频停滞快照。
///
/// 日志中的 `wait_queue` 可与 task 诊断里的
/// `Blocking(WaitQueue(...))` 直接对应。
pub fn log_debug_snapshot() {
    let snapshot = with_registry(|registry| registry.debug_snapshot());
    log::warn!("[stall-debug][futex] queues={} wait_attempts={} wait_returns={} wake_calls={} \
                woken_tasks={} requeue_calls={}",
               snapshot.queues
                       .len(),
               snapshot.wait_attempts,
               snapshot.wait_returns,
               snapshot.wake_calls,
               snapshot.woken_tasks,
               snapshot.requeue_calls);
    log::warn!("[stall-debug][futex] last_wait={:?} last_wait_result={:?} last_wake={:?} \
                last_requeue={:?}",
               snapshot.last_wait,
               snapshot.last_wait_result,
               snapshot.last_wake,
               snapshot.last_requeue);
    let active_queue_ids : alloc::vec::Vec<_> =
        snapshot.queues
                .iter()
                .map(|queue| (queue.wait_queue_id, queue.active_users))
                .collect();
    log::warn!("[stall-debug][futex] active_queue_ids={:?}", active_queue_ids);
}

/// 登记线程的 robust 链表头。
pub fn set_robust_list(task_id : TaskId, head : usize, len : usize) -> FutexResult<()> {
    if len != ROBUST_LIST_HEAD_SIZE {
        return Err(FutexError::Invalid);
    }
    with_registry(|registry| registry.set_robust_list(task_id, head, len));
    Ok(())
}

/// 查询线程的 robust 链表头；未登记时返回 `(0, 0)`。
pub fn get_robust_list(task_id : TaskId) -> (usize, usize) {
    with_registry(|registry| registry.robust_list(task_id))
}

/// 取出并删除线程的 robust 链表状态，供退出路径执行一次性清理。
pub fn take_robust_list(task_id : TaskId) -> (usize, usize) {
    with_registry(|registry| registry.take_robust_list(task_id))
}

/// 删除线程的 robust 链表状态。
pub fn drop_robust_list(task_id : TaskId) {
    with_registry(|registry| registry.drop_robust_list(task_id));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn robust_state_round_trip_and_take() {
        let task_id = usize::MAX - 1;
        set_robust_list(task_id, 0x1000, ROBUST_LIST_HEAD_SIZE).unwrap();
        assert_eq!(get_robust_list(task_id),
                   (0x1000, ROBUST_LIST_HEAD_SIZE));
        assert_eq!(take_robust_list(task_id),
                   (0x1000, ROBUST_LIST_HEAD_SIZE));
        assert_eq!(get_robust_list(task_id), (0, 0));
    }

    #[test]
    fn wake_missing_queue_is_empty() {
        assert_eq!(wake(FutexKey::private(0x2000, usize::MAX), 1),
                   0);
    }

    #[test]
    fn requeue_rejects_identical_keys() {
        let key = FutexKey::private(0x3000, usize::MAX);
        assert_eq!(requeue(key, key, 1, 1),
                   Err(FutexError::Invalid));
    }
}
