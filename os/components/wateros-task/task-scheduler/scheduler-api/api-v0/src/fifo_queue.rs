//!fifo queue

use core::array::from_fn;

use alloc::collections::vec_deque::VecDeque;
use task_api::{Priority, TaskId, BUCKET_COUNT};
pub struct FifoQueue {
    queues : [VecDeque<TaskId>; BUCKET_COUNT],
    task_count : usize,
}
impl FifoQueue {
    pub fn new() -> Self {
        Self { queues : from_fn(|_| VecDeque::new()),
               task_count : 0 }
    }
    pub fn init(&mut self) {
        for q in self.queues
                     .iter_mut()
        {
            q.clear();
        }
        self.task_count = 0;
    }
    /// 将一个任务加入队列。
    pub fn enqueue(&mut self, task_id : TaskId, priority : Priority) {
        let index = (priority - 1) as usize;
        self.queues[index].push_back(task_id);
        self.task_count = self.task_count
                              .saturating_add(1);
    }
    /// 从队列中移除一个任务。
    pub fn dequeue(&mut self, task_id : TaskId) {
        let mut removed = 0usize;
        for q in self.queues
                     .iter_mut()
        {
            let before = q.len();
            q.retain(|&task| task != task_id);
            removed = removed.saturating_add(before - q.len());
        }
        if removed != 0 {
            self.task_count = self.task_count
                                  .saturating_sub(removed);
        }
    }
    /// 从队列中选择下一个任务。
    pub fn pick(&mut self) -> Option<TaskId> {
        if self.task_count == 0 {
            return None;
        }
        for q in self.queues
                     .iter_mut()
                     .rev()
        {
            if let Some(task_id) = q.pop_front() {
                self.task_count = self.task_count
                                      .saturating_sub(1);
                return Some(task_id);
            }
        }
        None
    }
    pub fn pick_at_priority(&mut self, priority : Priority) -> Option<TaskId> {
        if self.task_count == 0 {
            return None;
        }
        let index = (priority - 1) as usize;
        let task_id = self.queues[index].pop_front()?;
        self.task_count = self.task_count
                              .saturating_sub(1);
        Some(task_id)
    }
    pub fn task_count(&self) -> usize { self.task_count }
    pub fn highest_priority(&self) -> Option<Priority> {
        for (i, q) in self.queues
                          .iter()
                          .enumerate()
                          .rev()
        {
            if !q.is_empty() {
                return Some((i + 1) as Priority);
            }
        }
        None
    }
}
