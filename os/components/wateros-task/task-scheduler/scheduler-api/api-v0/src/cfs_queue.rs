//!cfs队列实现

use alloc::collections::{btree_map::BTreeMap, vec_deque::VecDeque};
use task_api::{TaskId, VRunTime};

pub struct CfsQueue {
    tree : BTreeMap<VRunTime, VecDeque<TaskId>>,
    /// 单调不减的本 CPU CFS 基线。它在 ready tree 为空时仍然有效，
    /// 用于放置新建、唤醒和迁移进来的任务，避免它们因 vruntime=0 插队。
    min_vruntime : VRunTime,
    task_count : usize,
}
impl CfsQueue {
    pub fn new() -> Self {
        Self { tree : BTreeMap::new(),
               min_vruntime : 0,
               task_count : 0 }
    }
    pub fn init(&mut self) {
        self.tree.clear();
        self.min_vruntime = 0;
        self.task_count = 0;
    }
    /// 将任务 vruntime 钳制到本 CPU 的 CFS 基线。
    pub fn normalize_vruntime(&self, vruntime : VRunTime) -> VRunTime {
        vruntime.max(self.min_vruntime)
    }
    //任务入队
    pub fn enqueue(&mut self, task_id : TaskId, vruntime : VRunTime) {
        let vruntime = self.normalize_vruntime(vruntime);
        self.tree
            .entry(vruntime)
            .or_insert_with(VecDeque::new)
            .push_back(task_id);
        self.task_count = self.task_count
                              .saturating_add(1);
    }
    //任务出队
    pub fn dequeue(&mut self, task_id : TaskId) {
        let mut removed = 0usize;
        self.tree
            .retain(|_, tasks| {
                let before = tasks.len();
                tasks.retain(|id| *id != task_id);
                removed += before - tasks.len();
                !tasks.is_empty()
            });
        if removed != 0 {
            self.task_count = self.task_count
                                  .saturating_sub(removed);
        }
    }
    //选择下一个任务
    pub fn pick(&mut self) -> Option<TaskId> {
        if self.task_count == 0 {
            return None;
        }
        let mut entry = self.tree
                            .first_entry()?;
        self.min_vruntime = self.min_vruntime
                                .max(*entry.key());
        let task_id = entry.get_mut()
                           .pop_front();
        if entry.get()
                .is_empty()
        {
            entry.remove();
        }
        if task_id.is_some() {
            self.task_count = self.task_count
                                  .saturating_sub(1);
        }
        task_id
    }
    pub fn task_count(&self) -> usize { self.task_count }
    /// ready tree 中的最小 vruntime；用于判断当前任务是否应让出 CPU。
    pub fn min_ready_vruntime(&self) -> Option<VRunTime> {
        self.tree
            .keys()
            .next()
            .copied()
    }
    /// 以当前运行实体和 ready tree 的左端共同推进本 CPU 的 CFS 基线。
    ///
    /// 当前实体尚未进入 ready tree。若 tree 中仍有更落后的实体，不能直接将
    /// baseline 跳到当前实体的 vruntime，否则会抹去该实体应有的追赶份额。
    pub fn update_min_vruntime(&mut self, current_vruntime : VRunTime) {
        let candidate = self.min_ready_vruntime()
                            .map_or(current_vruntime, |ready_vruntime| {
                                current_vruntime.min(ready_vruntime)
                            });
        self.min_vruntime = self.min_vruntime
                                .max(candidate);
    }

    #[cfg(test)]
    fn min_vruntime(&self) -> VRunTime { self.min_vruntime }
}

#[cfg(test)]
mod tests {
    use super::CfsQueue;

    #[test]
    fn current_advances_baseline_when_ready_tree_is_empty() {
        let mut queue = CfsQueue::new();
        queue.update_min_vruntime(10);
        assert_eq!(queue.min_vruntime(), 10);
    }

    #[test]
    fn lagging_ready_entity_limits_baseline_advance() {
        let mut queue = CfsQueue::new();
        queue.enqueue(1, 5);
        queue.update_min_vruntime(10);
        assert_eq!(queue.min_vruntime(), 5);
    }

    #[test]
    fn enqueue_normalizes_to_baseline() {
        let mut queue = CfsQueue::new();
        queue.update_min_vruntime(10);
        queue.enqueue(1, 0);
        assert_eq!(queue.min_ready_vruntime(), Some(10));
    }

    #[test]
    fn equal_vruntime_preserves_fifo_order() {
        let mut queue = CfsQueue::new();
        queue.enqueue(1, 7);
        queue.enqueue(2, 7);
        assert_eq!(queue.pick(), Some(1));
        assert_eq!(queue.pick(), Some(2));
    }
}
