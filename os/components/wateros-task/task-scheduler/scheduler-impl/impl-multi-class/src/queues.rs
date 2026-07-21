//! `SCHED_OTHER` 专用就绪 FIFO。

extern crate alloc;

use alloc::collections::{BTreeMap, VecDeque};

use config::task::{MAX_TICKS_PER_TASK, READY_QUEUE_STALE_COMPACT_THRESHOLD};
use task_api::TaskId;

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
    current_ticks : u64,
}

impl OtherReadyQueue {
    pub(super) fn new() -> Self {
        Self { ready_queue : VecDeque::new(),
               current_ticks : 0,
               versions : BTreeMap::new() }
    }
    pub fn tick_current(&mut self) -> bool {
        self.current_ticks = self.current_ticks
                                 .saturating_add(1);
        self.current_ticks >= MAX_TICKS_PER_TASK
    }
    pub fn reset_ticks(&mut self) { self.current_ticks = 0; }
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
                versions.get(&entry.task_id)
                        .copied()
                        .is_some_and(|ver| ver == entry.version)
            });
    }

    fn stale_compact_threshold(&self) -> usize {
        READY_QUEUE_STALE_COMPACT_THRESHOLD.max(self.ready_queue
                                                    .len() /
                                                4)
    }

    pub(super) fn has_runnable(&self) -> bool {
        self.ready_queue
            .iter()
            .copied()
            .any(|entry| self.entry_is_live(entry))
    }

    /// 任务已从 registry 永久移除后回收 `versions` 条目。
    pub(super) fn forget_task(&mut self, task_id : TaskId) {
        self.versions
            .remove(&task_id);
    }

    pub fn enqueue_ready_task(&mut self, task_id : TaskId) {
        let version = self.bump_version(task_id);
        self.ready_queue
            .push_back(QueueEntry { task_id, version });
    }

    pub fn init(&mut self) {
        self.ready_queue
            .clear();
        self.versions
            .clear();
        self.current_ticks = 0;
    }

    pub fn detach_task(&mut self, task_id : TaskId) { let _ = self.bump_version(task_id); }

    pub fn pick_next_runnable_task_id(&mut self) -> Option<TaskId> {
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
            return Some(entry.task_id);
        }
        None
    }
}
