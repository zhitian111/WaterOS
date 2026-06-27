//! `SCHED_OTHER` 专用就绪 FIFO。

extern crate alloc;

use alloc::collections::{BTreeMap, VecDeque};
use api_v0::{ReadyQueue, ReadyTaskSink};
use task_api::{SchedulableCheck, TaskId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct QueueEntry {
    task_id : TaskId,
    version : u64,
}

/// `SCHED_OTHER` 任务的就绪队列。
///
/// 使用 per-task 版本号实现 O(1) 入队与去重：重复入队只需在队尾追加新条目并使旧条目失效，
/// 避免原先 `VecDeque::retain` 在大量并存线程下退化为 O(n^2)。
pub(super) struct OtherReadyQueue {
    ready_queue : VecDeque<QueueEntry>,
    versions : BTreeMap<TaskId, u64>,
}

impl OtherReadyQueue {
    pub(super) fn new() -> Self {
        Self { ready_queue : VecDeque::new(),
               versions : BTreeMap::new() }
    }

    fn entry_is_live(&self, entry : QueueEntry) -> bool {
        self.versions
            .get(&entry.task_id)
            .copied()
            .is_some_and(|ver| ver == entry.version)
    }

    fn bump_version(&mut self, task_id : TaskId) -> u64 {
        let entry = self.versions
                        .entry(task_id)
                        .or_insert(0);
        *entry = entry.saturating_add(1);
        *entry
    }

    pub(super) fn has_runnable(&self, check : &impl SchedulableCheck) -> bool {
        self.ready_queue
            .iter()
            .copied()
            .any(|entry| self.entry_is_live(entry) && check.is_schedulable(entry.task_id))
    }

    pub(super) fn debug_live_ids(&self) -> alloc::vec::Vec<TaskId> {
        self.ready_queue
            .iter()
            .copied()
            .filter(|entry| self.entry_is_live(*entry))
            .map(|entry| entry.task_id)
            .collect()
    }
}

impl ReadyTaskSink for OtherReadyQueue {
    fn enqueue_ready_task(&mut self, task_id : TaskId) {
        let version = self.bump_version(task_id);
        self.ready_queue
            .push_back(QueueEntry { task_id,
                                    version });
    }

    fn detach_ready_task(&mut self, task_id : TaskId) {
        let _ = self.bump_version(task_id);
    }
}

impl ReadyQueue for OtherReadyQueue {
    fn init(&mut self) {
        self.ready_queue
            .clear();
        self.versions
            .clear();
    }

    fn detach_task(&mut self, task_id : TaskId) {
        let _ = self.bump_version(task_id);
    }

    fn pick_next_runnable_task_id(&mut self, check : &impl SchedulableCheck) -> Option<TaskId> {
        while let Some(entry) = self.ready_queue
                                      .pop_front()
        {
            if !self.entry_is_live(entry) {
                continue;
            }
            if check.is_schedulable(entry.task_id) {
                return Some(entry.task_id);
            }
            log::trace!("[task-scheduler] skip unrunnable task {} in other ready_queue",
                        entry.task_id);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::collections::HashSet;

    struct MockCheck {
        live : HashSet<TaskId>,
    }

    impl MockCheck {
        fn new(ids : &[TaskId]) -> Self {
            Self { live : ids.iter()
                             .copied()
                             .collect() }
        }
    }

    impl SchedulableCheck for MockCheck {
        fn is_schedulable(&self, task_id : TaskId) -> bool {
            self.live
                .contains(&task_id)
        }
    }

    #[test]
    fn reenqueue_moves_task_to_back_without_linear_retain() {
        let mut q = OtherReadyQueue::new();
        let check = MockCheck::new(&[1, 2]);
        q.enqueue_ready_task(1);
        q.enqueue_ready_task(2);
        q.enqueue_ready_task(1);
        assert_eq!(q.pick_next_runnable_task_id(&check), Some(2));
        assert_eq!(q.pick_next_runnable_task_id(&check), Some(1));
        assert_eq!(q.pick_next_runnable_task_id(&check), None);
    }

    #[test]
    fn detach_invalidates_pending_entries() {
        let mut q = OtherReadyQueue::new();
        let check = MockCheck::new(&[1, 2]);
        q.enqueue_ready_task(1);
        q.enqueue_ready_task(2);
        q.detach_ready_task(1);
        assert_eq!(q.pick_next_runnable_task_id(&check), Some(2));
        assert_eq!(q.pick_next_runnable_task_id(&check), None);
    }
}
