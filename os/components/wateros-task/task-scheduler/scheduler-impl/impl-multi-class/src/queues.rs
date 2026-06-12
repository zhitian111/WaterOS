//! `SCHED_OTHER` 专用就绪 FIFO。

extern crate alloc;

use alloc::collections::VecDeque;
use api_v0::{ReadyQueue, ReadyTaskSink};
use task_api::{SchedulableCheck, TaskId};

/// `SCHED_OTHER` 任务的就绪队列。
pub(super) struct OtherReadyQueue {
    ready_queue : VecDeque<TaskId>,
}

impl OtherReadyQueue {
    pub(super) fn new() -> Self { Self { ready_queue : VecDeque::new() } }

    pub(super) fn has_runnable(&self, check : &impl SchedulableCheck) -> bool {
        self.ready_queue
            .iter()
            .copied()
            .any(|task_id| check.is_schedulable(task_id))
    }
}

impl ReadyTaskSink for OtherReadyQueue {
    fn enqueue_ready_task(&mut self, task_id : TaskId) {
        self.ready_queue
            .push_back(task_id);
    }
}

impl ReadyQueue for OtherReadyQueue {
    fn init(&mut self) {
        self.ready_queue
            .clear();
    }

    fn detach_task(&mut self, task_id : TaskId) {
        let _ = remove_task_id(&mut self.ready_queue, task_id);
    }

    fn pick_next_runnable_task_id(&mut self, check : &impl SchedulableCheck) -> Option<TaskId> {
        while let Some(task_id) = self.ready_queue
                                      .pop_front()
        {
            if check.is_schedulable(task_id) {
                return Some(task_id);
            }
            log::trace!("[task-scheduler] skip unrunnable task {} in other ready_queue",
                        task_id);
        }
        None
    }
}

fn remove_task_id(queue : &mut VecDeque<TaskId>, task_id : TaskId) -> bool {
    let old_len = queue.len();
    queue.retain(|candidate| *candidate != task_id);
    queue.len() != old_len
}
