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
pub(super) struct OtherReadyQueue {
    ready_queue : VecDeque<QueueEntry>,
    versions : BTreeMap<TaskId, u64>,
}

impl OtherReadyQueue {
    pub(super) fn new() -> Self {
        Self { ready_queue : VecDeque::new(),
               versions : BTreeMap::new() }
    }

    pub(super) fn ready_queue_len(&self) -> usize { self.ready_queue.len() }

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
