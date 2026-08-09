//!rr queue

use core::array;

use alloc::collections::vec_deque::VecDeque;
use task_api::Priority;
use task_api::TaskId;
use task_api::BUCKET_COUNT;
pub struct RrQueue {
    queues : [VecDeque<TaskId>; BUCKET_COUNT],
    current_ticks : u64,
    task_count : usize,
}
impl RrQueue {
    pub fn new() -> Self {
        Self { queues : array::from_fn(|_| VecDeque::new()),
               current_ticks : 0,
               task_count : 0 }
    }
    pub fn init(&mut self) {
        for q in self.queues
                     .iter_mut()
        {
            q.clear();
        }
        self.current_ticks = 0;
        self.task_count = 0;
    }
    pub fn enqueue(&mut self, task_id : TaskId, priority : Priority) {
        let index = (priority - 1) as usize;
        self.queues[index].push_back(task_id);
        self.task_count = self.task_count
                              .saturating_add(1);
    }
    pub fn dequeue(&mut self, task_id : TaskId) {
        let mut removed = 0usize;
        for q in self.queues
                     .iter_mut()
        {
            let before = q.len();
            q.retain(|&id| id != task_id);
            removed = removed.saturating_add(before - q.len());
        }
        if removed != 0 {
            self.task_count = self.task_count
                                  .saturating_sub(removed);
        }
    }
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
    pub fn reset_tick(&mut self) { self.current_ticks = 0; }
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
    /// 查看指定优先级队首任务而不出队；用于负载均衡的偷取判断。
    pub fn front_at_priority(&self, priority : Priority) -> Option<TaskId> {
        let index = (priority - 1) as usize;
        self.queues[index].front()
                          .copied()
    }
    pub fn task_count(&self) -> usize { self.task_count }
    pub fn highest_priority(&self) -> Option<Priority> {
        if self.task_count == 0 {
            return None;
        }
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
