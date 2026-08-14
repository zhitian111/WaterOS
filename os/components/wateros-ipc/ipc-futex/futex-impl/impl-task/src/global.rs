//! 全局 futex 服务。
//!
//! ARCH: 本文件是 IPC 与 task scheduler 的边界。它只维护 futex 元数据，
//! 通过 [`ipc_waitqueue::WaitQueue`] 请求阻塞或唤醒，绝不直接选择 CPU 或发送 IPI。
//!
//! 公开函数隐藏 registry 与锁。等待和唤醒期间不会长期持有 registry 锁；
//! `active_users` 保护锁外 scheduler 操作所引用的等待队列。

use api_v0::{
    FutexError, FutexKey, FutexResult, FutexWaitOutcome, RobustListRegistration,
    ROBUST_LIST_HEAD_SIZE,
};
use core::sync::atomic::Ordering;
use spin::Mutex;
use task_api::{TaskId, TaskTick, TaskWaitResult};

use crate::registry::FutexRegistry;

/// 唯一的 futex 状态实例；registry 和锁均不跨越本模块边界。
///
/// LOCK: 此锁只保护 [`FutexRegistry`] 元数据。拿到 `WaitQueue` 后必须先释放
/// 此锁，再调用可能进入 scheduler 的 wait/wake/requeue 操作。
static REGISTRY : Mutex<FutexRegistry> = Mutex::new(FutexRegistry::new());

/// 在短暂 registry 临界区内访问 futex 元数据。
///
/// LOCK: `f` 不得阻塞、调度、访问用户内存或重入本模块的公开 futex 接口，
/// 否则可能形成 registry 锁重入或锁顺序问题。
fn with_registry<R>(f : impl FnOnce(&mut FutexRegistry) -> R) -> R {
    let cpu = arch::cpu::current_cpu_id().raw();
    let object = &REGISTRY as *const _ as usize;
    let mut registry = if debug::ENABLED {
        if let Some(registry) = REGISTRY.try_lock() {
            registry
        } else {
            debug::lock_wait(cpu,
                             0,
                             debug::NO_TASK,
                             debug::DebugLockKind::FutexRegistry,
                             object);
            REGISTRY.lock()
        }
    } else {
        REGISTRY.lock()
    };
    debug::lock_acquired(cpu, debug::DebugLockKind::FutexRegistry, object);
    let result = f(&mut registry);
    drop(registry);
    debug::lock_released(cpu, debug::DebugLockKind::FutexRegistry, object);
    result
}

/// 在 `key` 对应队列上等待，阻塞前通过 `condition` 再次确认用户态条件。
///
/// FLOW: 条件预检 -> 登记 waiter / 取得队列 -> 条件复检 -> scheduler 等待 ->
/// 取消登记。`wake_sequence` 覆盖“条件复检到真正睡眠”之间的 lost-wake 窗口。
///
/// Concurrency: `condition` 在 scheduler 锁外执行；调用方负责安全读取用户 futex
/// 字并把访问错误转换为其自己的错误路径。
pub fn wait_while(task_id : TaskId,
                  key : FutexKey,
                  bitset : u32,
                  timeout : Option<TaskTick>,
                  mut condition : impl FnMut() -> bool)
                  -> FutexWaitOutcome {
    let cpu = arch::cpu::current_cpu_id().raw();
    debug::record_event(cpu,
                        0,
                        task_id as u64,
                        debug::DebugEventKind::FutexWait,
                        0,
                        [key.uaddr as u64, key.private_scope as u64, timeout.unwrap_or(u64::MAX)]);
    if !condition() {
        return FutexWaitOutcome::ConditionChanged;
    }

    let (wait_queue, wake_sequence, observed_wake, waiter_sequence, observed_waiter_wake) =
        with_registry(|registry| {
        let (wait_queue, wake_sequence) = registry.acquire_queue(key);
        let waiter_sequence = registry.register_waiting_task(task_id, key, bitset);
        registry.record_wait_attempt(key, wait_queue.id());
        // The registry lock linearizes waiter publication with wakers obtaining
        // this queue. Loading after unlock would let a concurrent wake become
        // the waiter's baseline and then be lost before scheduler enqueue.
        let observed_wake = wake_sequence.load(Ordering::Acquire);
        let observed_waiter_wake = waiter_sequence.load(Ordering::Acquire);
        (wait_queue, wake_sequence, observed_wake, waiter_sequence, observed_waiter_wake)
    });
    if !condition() {
        with_registry(|registry| {
            registry.record_wait_result(key, FutexWaitOutcome::ConditionChanged);
            registry.finish_waiting_task(task_id, key);
        });
        return FutexWaitOutcome::ConditionChanged;
    }

    // LOCK: condition 已在 scheduler 锁外复查。进入调度临界区后只比较原子 wake
    // 序列：若 wake 发生在“复查—入队”窗口，序列变化会阻止当前任务睡眠；
    // 若 wake 稍后发生，唤醒者会等待 scheduler 锁并看到已入队任务。
    let not_woken = || wake_sequence.load(Ordering::Acquire) == observed_wake &&
                         waiter_sequence.load(Ordering::Acquire) == observed_waiter_wake;
    let outcome = match timeout {
        None => match wait_queue.wait_current_while(not_woken) {
            TaskWaitResult::Interrupted => FutexWaitOutcome::Interrupted,
            _ => FutexWaitOutcome::Woken,
        },
        Some(0) => {
            if condition() {
                FutexWaitOutcome::TimedOut
            } else {
                FutexWaitOutcome::ConditionChanged
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
        registry.finish_waiting_task(task_id, key);
    });
    outcome
}

/// 任务在 futex syscall 返回前被终止时，代其释放 registry 使用权。
///
/// FLOW: 这是正常 `wait_while` 收尾之外的退出/异常回滚路径；幂等调用。
pub fn cancel_task_wait(task_id : TaskId) {
    with_registry(|registry| registry.cancel_waiting_task(task_id));
}

/// 唤醒 `key` 队列上最多 `max_wake` 个等待者。
///
/// SMP: 被唤醒任务的目标 CPU 和定向重调度 IPI 由 `wateros-task` 决定；本函数
/// 仅对 WaitQueue 发出唤醒请求。
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

    // FLOW: 必须先发布序列变化、后执行 scheduler wake；并发 waiter 因而不会在
    // 观察到旧序列后错过这次唤醒。
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
    debug::record_event(arch::cpu::current_cpu_id().raw(),
                        0,
                        debug::NO_TASK,
                        debug::DebugEventKind::FutexWake,
                        0,
                        [key.uaddr as u64, max_wake as u64, woken as u64]);
    woken
}

/// 仅唤醒等待 bitset 与 `wake_bitset` 有交集的 waiter。
///
/// 每个 waiter 使用独立序列覆盖“已登记但尚未进入 scheduler 队列”的窗口；
/// 因而不能复用普通 wake 的整队列序列，否则不匹配的 waiter 也会被放行。
pub fn wake_bitset(key : FutexKey, max_wake : u32, wake_bitset : u32) -> usize {
    if max_wake == 0 || wake_bitset == 0 {
        return 0;
    }
    let Some((wait_queue, waiters)) = with_registry(|registry| {
        let (wait_queue, _) = registry.acquire_existing_queue(key)?;
        let waiters = registry.matching_waiters(key, wake_bitset, max_wake);
        Some((wait_queue, waiters))
    }) else {
        return 0;
    };
    let selected = waiters.len();
    for (task_id, wake_sequence) in waiters {
        wake_sequence.fetch_add(1, Ordering::Release);
        // false 也可能表示 waiter 正位于“登记—scheduler 入队”窗口；独立
        // sequence 已保证它不会继续睡眠，因此仍计入本次 wake 返回值。
        let _ = wait_queue.wake_task(task_id);
    }
    with_registry(|registry| {
        registry.record_wake(key, max_wake, selected);
        registry.release_queue(key);
    });
    selected
}

/// 唤醒 `key` 队列上的全部等待者。
///
/// Concurrency: 与 [`wake`] 使用相同的序列发布和 `active_users` 生命周期约束。
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
    debug::record_event(arch::cpu::current_cpu_id().raw(),
                        0,
                        debug::NO_TASK,
                        debug::DebugEventKind::FutexWake,
                        0,
                        [key.uaddr as u64, u32::MAX as u64, woken as u64]);
    woken
}

/// 唤醒 `from_key` 上的部分等待者，并把后续等待者迁移到 `to_key`。
///
/// FLOW: 不带用户字比较的 requeue；源和目标相同会被拒绝，避免自迁移破坏
/// WaitQueue 的队列顺序。
pub fn requeue(from_key : FutexKey,
               to_key : FutexKey,
               wake_count : u32,
               requeue_count : u32)
               -> FutexResult<usize> {
    match requeue_if(from_key,
                     to_key,
                     wake_count,
                     requeue_count,
                     || Ok(true))?
    {
        Some(changed) => Ok(changed),
        None => Ok(0),
    }
}

/// 比较条件与队列迁移在同一个 scheduler 临界区内完成。
///
/// LOCK: `condition` 由 WaitQueue 在其 scheduler 临界区调用。调用方只能做不会
/// 阻塞、不会重入 futex 的条件读取；不匹配时返回 [`FutexError::Again`]。
pub fn cmp_requeue(from_key : FutexKey,
                   to_key : FutexKey,
                   wake_count : u32,
                   requeue_count : u32,
                   condition : impl FnOnce() -> FutexResult<bool>)
                   -> FutexResult<usize> {
    match requeue_if(from_key,
                     to_key,
                     wake_count,
                     requeue_count,
                     condition)?
    {
        Some(changed) => Ok(changed),
        None => Err(FutexError::Again),
    }
}

/// `requeue` 与 `cmp_requeue` 的共同实现。
///
/// FLOW: 持 registry 锁取得两个队列并增加使用权 -> 解锁后完成 scheduler
/// requeue -> 回到 registry 锁记录结果并对两个 key 各释放一次使用权。
fn requeue_if(from_key : FutexKey,
              to_key : FutexKey,
              wake_count : u32,
              requeue_count : u32,
              condition : impl FnOnce() -> FutexResult<bool>)
              -> FutexResult<Option<usize>> {
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
        return condition().map(|matched| matched.then_some(0));
    };

    let mut condition_result = None;
    let result = from_queue.requeue_to_detailed_while(to_queue,
                                    wake_count as usize,
                                    requeue_count as usize,
                                    || {
                                        let result = condition();
                                        let matched = matches!(result, Ok(true));
                                        condition_result = Some(result);
                                        if matched {
                                            // SMP: 与 waiter 的 scheduler 临界区复查串行化：
                                            // 成功 requeue 后，尚未真正入队的 waiter
                                            // 会观察到序列变化而不会睡回源队列。
                                            from_wake_sequence.fetch_add(1, Ordering::Release);
                                        }
                                        matched
                                    });
    let changed = result.as_ref().map(|result| result.changed());
    with_registry(|registry| {
        if let Some(result) = result.as_ref() {
            registry.migrate_waiting_tasks(&result.moved, from_key, to_key);
        }
        registry.record_requeue(from_key,
                                to_key,
                                wake_count,
                                requeue_count,
                                changed.unwrap_or(0));
        registry.release_queue(from_key);
        registry.release_queue(to_key);
    });
    match condition_result {
        Some(Ok(_)) => Ok(changed),
        Some(Err(error)) => Err(error),
        None => Err(FutexError::Invalid),
    }
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
    log::warn!("[stall-debug][futex] active_queue_ids={:?}",
               active_queue_ids);
}

/// 登记线程的 robust 链表头。
pub fn set_robust_list(task_id : TaskId,
                       head : usize,
                       len : usize,
                       user_aspace : usize)
                       -> FutexResult<()> {
    if len != ROBUST_LIST_HEAD_SIZE {
        return Err(FutexError::Invalid);
    }
    with_registry(|registry| registry.set_robust_list(task_id, head, len, user_aspace));
    Ok(())
}

/// 查询线程的 robust 链表头；未登记时返回空指针和 ABI 规定的头长度。
pub fn get_robust_list(task_id : TaskId) -> (usize, usize) {
    with_registry(|registry| registry.robust_list(task_id))
}

/// 取出并删除线程的 robust 链表状态，供退出路径执行一次性清理。
pub fn take_robust_list(task_id : TaskId) -> Option<RobustListRegistration> {
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
        set_robust_list(task_id,
                        0x1000,
                        ROBUST_LIST_HEAD_SIZE,
                        0x2000).unwrap();
        assert_eq!(get_robust_list(task_id),
                   (0x1000, ROBUST_LIST_HEAD_SIZE));
        assert_eq!(take_robust_list(task_id),
                   Some(RobustListRegistration { head : 0x1000,
                                                 len : ROBUST_LIST_HEAD_SIZE,
                                                 user_aspace : 0x2000 }));
        assert_eq!(get_robust_list(task_id),
                   (0, ROBUST_LIST_HEAD_SIZE));
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
