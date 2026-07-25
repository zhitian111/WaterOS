//!rr queue

use core::array;

use alloc::collections::vec_deque::VecDeque;
use config::task::MAX_TICKS_PER_TASK;
use task_api::Priority;
use task_api::TaskId;
use task_api::BUCKET_COUNT;
struct RrQueue {
    queues : [VecDeque<TaskId>; BUCKET_COUNT],
    current_ticks : u64,
}
impl RrQueue {
    pub fn new() -> Self {
        Self { queues : array::from_fn(|_| VecDeque::new()),
               current_ticks : 0 }
    }
    pub fn init(&mut self) {
        for q in self.queues
                     .iter_mut()
        {
            q.clear();
        }
        self.current_ticks = 0;
    }
    pub fn enqueue(&mut self, task_id : TaskId, priority : Priority) {
        let index = (priority - 1) as usize;
        self.queues[index].push_back(task_id);
    }
    pub fn dequeue(&mut self, task_id : TaskId) {
        for q in self.queues
                     .iter_mut()
        {
            q.retain(|&id| id != task_id);
        }
    }
    pub fn pick(&mut self) -> Option<TaskId> {
        for q in self.queues
                     .iter_mut()
                     .rev()
        {
            if let Some(task_id) = q.pop_front() {
                q.push_back(task_id);
                return Some(task_id);
            }
        }
        None
    }
    pub fn tick(&mut self) -> bool {
        self.current_ticks = self.current_ticks
                                 .saturating_add(1);
        if self.current_ticks >= MAX_TICKS_PER_TASK {
            return true;
        }
        false
    }
    pub fn reset_tick(&mut self) { self.current_ticks = 0; }
    pub fn pick_at_priority(&mut self, priority : Priority) -> Option<TaskId> {
        let index = (priority - 1) as usize;
        let task_id = self.queues[index].pop_front()?;
        self.queues[index].push_back(task_id);
        Some(task_id)
    }
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
    pub fn is_current_runnable(&self) -> bool { self.current_ticks < MAX_TICKS_PER_TASK }
}
