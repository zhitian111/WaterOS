//! `SCHED_OTHER` 专用就绪 FIFO。

extern crate alloc;

use alloc::collections::{BTreeMap, VecDeque};
use api_v0::{ReadyQueue, ReadyTaskSink};
use config::task::READY_QUEUE_STALE_COMPACT_THRESHOLD;
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

    fn compact_stale_entries(&mut self) {
        let versions = &self.versions;
        self.ready_queue
            .retain(|entry| {
                versions
                    .get(&entry.task_id)
                    .copied()
                    .is_some_and(|ver| ver == entry.version)
            });
    }

    fn stale_compact_threshold(&self) -> usize {
        READY_QUEUE_STALE_COMPACT_THRESHOLD.max(self.ready_queue
                                                    .len()
                                                    / 4)
    }

    pub(super) fn has_runnable(&self, check : &impl SchedulableCheck) -> bool {
        self.ready_queue
            .iter()
            .copied()
            .any(|entry| self.entry_is_live(entry) && check.is_schedulable(entry.task_id))
    }

    /// 任务已从 registry 永久移除后回收 `versions` 条目。
    pub(super) fn forget_task(&mut self, task_id : TaskId) {
        self.versions
            .remove(&task_id);
    }

    #[cfg(test)]
    fn versions_len(&self) -> usize { self.versions.len() }

    #[cfg(test)]
    fn versions_contains(&self, task_id : TaskId) -> bool { self.versions.contains_key(&task_id) }

    #[cfg(test)]
    fn ready_queue_len(&self) -> usize { self.ready_queue.len() }
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
        let mut consecutive_stale = 0usize;
        while let Some(entry) = self.ready_queue
                                      .pop_front()
        {
            if !self.entry_is_live(entry) {
                consecutive_stale = consecutive_stale.saturating_add(1);
                if consecutive_stale >= self.stale_compact_threshold() {
                    self.compact_stale_entries();
                    consecutive_stale = 0;
                }
                continue;
            }
            consecutive_stale = 0;
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

    #[test]
    fn compact_drops_stale_entries_after_threshold() {
        let mut q = OtherReadyQueue::new();
        let check = MockCheck::new(&[1]);
        for _ in 0..READY_QUEUE_STALE_COMPACT_THRESHOLD + 4 {
            q.enqueue_ready_task(1);
            q.detach_ready_task(1);
        }
        q.enqueue_ready_task(1);
        assert!(q.ready_queue_len() > READY_QUEUE_STALE_COMPACT_THRESHOLD);
        assert_eq!(q.pick_next_runnable_task_id(&check), Some(1));
        assert!(q.ready_queue_len() <= 2);
    }

    #[test]
    fn forget_task_removes_versions_entry() {
        let mut q = OtherReadyQueue::new();
        let check = MockCheck::new(&[1]);
        q.enqueue_ready_task(1);
        assert!(q.versions_contains(1));
        q.forget_task(1);
        assert!(!q.versions_contains(1));
        assert_eq!(q.pick_next_runnable_task_id(&check), None);
    }

    #[test]
    fn detach_does_not_remove_versions_entry() {
        let mut q = OtherReadyQueue::new();
        q.enqueue_ready_task(1);
        q.detach_ready_task(1);
        assert!(q.versions_contains(1));
    }

    #[test]
    fn versions_bounded_after_forget_cycle() {
        let mut q = OtherReadyQueue::new();
        for task_id in 1..=64u64 {
            q.enqueue_ready_task(task_id);
            q.forget_task(task_id);
        }
        assert_eq!(q.versions_len(), 0);
    }
}
