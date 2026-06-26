//! 基于 `ipc-waitqueue` 的全局 futex 枢纽。

extern crate alloc;

use alloc::collections::BTreeMap;

use api_v0::{FutexError, FutexKey, FutexResult, FutexWaitOutcome, KernelFutexOps, ROBUST_LIST_HEAD_SIZE};
use ipc_waitqueue::WaitQueue;
use spin::Mutex;
use task_api::{TaskId, TaskTick, TaskWaitResult};

use crate::robust::RobustState;

struct FutexTables {
    queues: BTreeMap<FutexKey, WaitQueue>,
    robust: BTreeMap<TaskId, RobustState>,
}

/// 全局 futex 表：等待队列 + per-task robust 状态。
pub struct FutexHub {
    inner: Mutex<FutexTables>,
}

impl FutexHub {
    /// 返回全局 futex 枢纽单例。
    pub fn global() -> &'static Self {
        &GLOBAL_HUB
    }

    fn with_tables<R>(&self, f: impl FnOnce(&mut FutexTables) -> R) -> R {
        let mut guard = self.inner.lock();
        f(&mut guard)
    }

    fn get_queue(tables: &mut FutexTables, key: FutexKey) -> WaitQueue {
        *tables
            .queues
            .entry(key)
            .or_insert_with(WaitQueue::new)
    }

    fn cleanup_empty_queue(tables: &mut FutexTables, key: FutexKey) {
        let Some(wq) = tables.queues.get(&key).copied() else {
            return;
        };
        if wq.try_release_empty() {
            tables.queues.remove(&key);
        }
    }

    /// 在 `key` 对应队列上带条件等待；用户内存复查由 `condition` 闭包完成（S1）。
    pub fn wait_while(
        &self,
        key: FutexKey,
        timeout: Option<TaskTick>,
        mut condition: impl FnMut() -> bool,
    ) -> FutexWaitOutcome {
        if !condition() {
            return FutexWaitOutcome::Woken;
        }
        let wq = self.with_tables(|tables| Self::get_queue(tables, key));
        if !condition() {
            self.with_tables(|tables| Self::cleanup_empty_queue(tables, key));
            return FutexWaitOutcome::Woken;
        }
        let outcome = match timeout {
            None => {
                match wq.wait_current_while(|| condition()) {
                    TaskWaitResult::Interrupted => FutexWaitOutcome::Interrupted,
                    _ => FutexWaitOutcome::Woken,
                }
            }
            Some(0) => {
                if condition() {
                    FutexWaitOutcome::TimedOut
                } else {
                    FutexWaitOutcome::Woken
                }
            }
            Some(ticks) => match wq.wait_current_while_for_ticks(ticks, || condition()) {
                TaskWaitResult::Woken => FutexWaitOutcome::Woken,
                TaskWaitResult::TimedOut => FutexWaitOutcome::TimedOut,
                TaskWaitResult::Interrupted => FutexWaitOutcome::Interrupted,
            },
        };
        self.with_tables(|tables| Self::cleanup_empty_queue(tables, key));
        outcome
    }

    /// 唤醒 `from_key` 上的前 `wake_count` 个等待者，并把后续等待者迁移到
    /// `to_key` 队列；返回被唤醒和被迁移的总数。
    pub fn requeue(&self,
                   from_key : FutexKey,
                   to_key : FutexKey,
                   wake_count : u32,
                   requeue_count : u32)
                   -> FutexResult<usize> {
        Ok(self.with_tables(|tables| {
            let from_wq = Self::get_queue(tables, from_key);
            let to_wq = Self::get_queue(tables, to_key);
            let changed = from_wq.requeue_to(to_wq, wake_count as usize, requeue_count as usize);
            Self::cleanup_empty_queue(tables, from_key);
            Self::cleanup_empty_queue(tables, to_key);
            changed
        }))
    }
}

static GLOBAL_HUB: FutexHub = FutexHub {
    inner: Mutex::new(FutexTables {
        queues: BTreeMap::new(),
        robust: BTreeMap::new(),
    }),
};

impl KernelFutexOps for FutexHub {
    fn wake(&self, key: FutexKey, max_wake: u32) -> FutexResult<usize> {
        let wq = self.with_tables(|tables| tables.queues.get(&key).copied());
        let Some(wq) = wq else {
            return Ok(0);
        };
        let limit = if max_wake == 0 { 1 } else { max_wake as usize };
        let mut woken = 0usize;
        for _ in 0..limit {
            if wq.wake_one().is_none() {
                break;
            }
            woken += 1;
        }
        self.with_tables(|tables| Self::cleanup_empty_queue(tables, key));
        Ok(woken)
    }

    fn wake_all(&self, key: FutexKey) -> FutexResult<usize> {
        let wq = self.with_tables(|tables| tables.queues.get(&key).copied());
        let woken = wq.map(|wq| wq.wake_all()).unwrap_or(0);
        self.with_tables(|tables| Self::cleanup_empty_queue(tables, key));
        Ok(woken)
    }

    fn set_robust_list(&self, task: TaskId, head: usize, len: usize) -> FutexResult<()> {
        if len != ROBUST_LIST_HEAD_SIZE {
            return Err(FutexError::Invalid);
        }
        self.with_tables(|tables| {
            tables.robust.insert(
                task,
                RobustState { head, len },
            );
        });
        Ok(())
    }

    fn get_robust_list(&self, task: TaskId) -> FutexResult<(usize, usize)> {
        Ok(self.with_tables(|tables| {
            tables
                .robust
                .get(&task)
                .map(|state| (state.head, state.len))
                .unwrap_or((0, 0))
        }))
    }

    fn drop_robust_list(&self, task: TaskId) {
        self.with_tables(|tables| {
            tables.robust.remove(&task);
        });
    }
}
