//!fifo queue

use core::array::from_fn;

use alloc::collections::{btree_map::BTreeMap, vec_deque::VecDeque};
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
        for q in self.queues
                     .iter_mut()
        {
            q.retain(|&task| task != task_id);
        }
        self.task_count = self.task_count
                              .saturating_sub(1);
    }
    /// 从队列中选择下一个任务。
    pub fn pick(&mut self) -> Option<TaskId> {
        if self.task_count == 0 {
            return None;
        }
        self.task_count = self.task_count
                              .saturating_sub(1);
        for q in self.queues
                     .iter_mut()
                     .rev()
        {
            if let Some(task_id) = q.pop_front() {
                return Some(task_id);
            }
        }
        None
    }
    pub fn pick_at_priority(&mut self, priority : Priority) -> Option<TaskId> {
        if self.task_count == 0 {
            return None;
        }
        self.task_count = self.task_count
                              .saturating_sub(1);
        let index = (priority - 1) as usize;
        self.queues[index].pop_front()
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
