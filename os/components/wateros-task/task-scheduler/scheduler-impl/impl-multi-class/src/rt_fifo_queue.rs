//! `SCHED_FIFO` 就绪队列：99 个优先级桶，每桶 FIFO，无时间片。

extern crate alloc;

use alloc::collections::VecDeque;
use task_api::{SchedulableCheck, TaskId};

const RT_PRIORITY_MIN : i32 = 1;
const RT_PRIORITY_MAX : i32 = 99;
const RT_BUCKET_COUNT : usize = (RT_PRIORITY_MAX - RT_PRIORITY_MIN + 1) as usize;

fn bucket_index(priority : i32) -> Option<usize> {
    if (RT_PRIORITY_MIN..=RT_PRIORITY_MAX).contains(&priority) {
        Some((priority - RT_PRIORITY_MIN) as usize)
    } else {
        None
    }
}

/// `SCHED_FIFO` 就绪队列。
pub struct RtFifoRunQueue {
    buckets : [VecDeque<TaskId>; RT_BUCKET_COUNT],
}

impl RtFifoRunQueue {
    /// 构造空队列。
    pub fn new() -> Self { Self { buckets : core::array::from_fn(|_| VecDeque::new()) } }

    /// 按优先级将任务入队尾。
    pub fn enqueue(&mut self, task_id : TaskId, priority : i32) {
        if let Some(index) = bucket_index(priority) {
            self.buckets[index].push_back(task_id);
        }
    }

    /// 从最高优先级非空桶弹出首个可调度任务。
    pub fn pick_next(&mut self, check : &impl SchedulableCheck) -> Option<TaskId> {
        for bucket in self.buckets
                          .iter_mut()
                          .rev()
        {
            while let Some(task_id) = bucket.pop_front() {
                if check.is_schedulable(task_id) {
                    return Some(task_id);
                }
            }
        }
        None
    }

    /// 从所有桶移除任务。
    pub fn remove(&mut self, task_id : TaskId) {
        for bucket in &mut self.buckets {
            let _ = take_task_id_by_id(bucket, task_id);
        }
    }

    /// 任务解除阻塞后重新入队。
    pub fn on_task_unblocked(&mut self, task_id : TaskId, priority : i32) {
        self.enqueue(task_id, priority);
    }

    /// 返回当前最高的可运行优先级，不改变队列内容。
    pub fn highest_runnable_priority(&self, check : &impl SchedulableCheck) -> Option<i32> {
        self.buckets
            .iter()
            .enumerate()
            .rev()
            .find(|(_, bucket)| {
                bucket.iter()
                      .copied()
                      .any(|task_id| check.is_schedulable(task_id))
            })
            .map(|(index, _)| (index as i32) + RT_PRIORITY_MIN)
    }

    /// 从指定优先级桶弹出首个可调度任务（供 multi-class 按优先级扫描）。
    pub fn pop_front_at_priority(&mut self,
                                 priority : i32,
                                 check : &impl SchedulableCheck)
                                 -> Option<TaskId> {
        let index = bucket_index(priority)?;
        while let Some(task_id) = self.buckets[index].pop_front() {
            if check.is_schedulable(task_id) {
                return Some(task_id);
            }
        }
        None
    }
}

fn take_task_id_by_id(queue : &mut VecDeque<TaskId>, task_id : TaskId) -> bool {
    if let Some(pos) = queue.iter()
                          .position(|candidate| *candidate == task_id)
    {
        queue.remove(pos);
        true
    } else {
        false
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
    fn pick_next_orders_by_priority() {
        let mut q = RtFifoRunQueue::new();
        q.enqueue(1, 1);
        q.enqueue(2, 50);
        q.enqueue(3, 99);
        let check = MockCheck::new(&[1, 2, 3]);
        assert_eq!(q.pick_next(&check), Some(3));
        assert_eq!(q.pick_next(&check), Some(2));
        assert_eq!(q.pick_next(&check), Some(1));
        assert_eq!(q.pick_next(&check), None);
    }

    #[test]
    fn highest_priority_does_not_consume_queue() {
        let mut q = RtFifoRunQueue::new();
        q.enqueue(1, 10);
        q.enqueue(2, 80);
        let check = MockCheck::new(&[1, 2]);
        assert_eq!(q.highest_runnable_priority(&check),
                   Some(80));
        assert_eq!(q.pick_next(&check), Some(2));
    }
}
