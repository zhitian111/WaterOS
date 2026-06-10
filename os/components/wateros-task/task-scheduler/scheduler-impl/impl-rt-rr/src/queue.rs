//! `SCHED_RR` 就绪队列：优先级桶 + 同优先级时间片轮转。

extern crate alloc;

use alloc::collections::VecDeque;
use config::task::MAX_RT_TICKS_PER_TASK;
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

fn priority_from_index(index : usize) -> i32 {
    (index as i32) + RT_PRIORITY_MIN
}

/// RR tick 处理结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RrTickAction {
    /// 继续运行当前 RR 任务。
    ContinueRunning,
    /// 时间片耗尽，让出给同优先级下一个 RR 任务。
    YieldToSamePriority,
}

/// `SCHED_RR` 就绪队列。
pub struct RtRrRunQueue {
    buckets : [VecDeque<TaskId>; RT_BUCKET_COUNT],
    current : Option<(TaskId, i32)>,
    remaining_ticks : u64,
}

impl RtRrRunQueue {
    /// 构造空队列。
    pub fn new() -> Self {
        Self { buckets : core::array::from_fn(|_| VecDeque::new()),
               current : None,
               remaining_ticks : 0 }
    }

    /// 按优先级将任务入队尾。
    pub fn enqueue(&mut self, task_id : TaskId, priority : i32) {
        if let Some(index) = bucket_index(priority) {
            self.buckets[index].push_back(task_id);
        }
    }

    /// 选取下一个可运行 RR 任务；若当前任务仍有时间片则继续运行。
    pub fn pick_next(&mut self, check : &impl SchedulableCheck) -> Option<TaskId> {
        if let Some((task_id, _priority)) = self.current {
            if check.is_schedulable(task_id) && self.remaining_ticks > 0 {
                return Some(task_id);
            }
            self.current = None;
            self.remaining_ticks = 0;
        }
        for (index, bucket) in self.buckets.iter_mut().enumerate().rev() {
            while let Some(task_id) = bucket.pop_front() {
                if check.is_schedulable(task_id) {
                    let priority = priority_from_index(index);
                    self.current = Some((task_id, priority));
                    self.remaining_ticks = MAX_RT_TICKS_PER_TASK;
                    return Some(task_id);
                }
            }
        }
        None
    }

    /// 时钟 tick：处理当前 RR 任务时间片。
    pub fn on_tick_current(&mut self, current : TaskId, priority : i32) -> RrTickAction {
        if self.current != Some((current, priority)) {
            self.current = Some((current, priority));
            self.remaining_ticks = MAX_RT_TICKS_PER_TASK;
        }
        if self.remaining_ticks <= 1 {
            self.remaining_ticks = 0;
            if let Some(index) = bucket_index(priority) {
                self.buckets[index].push_back(current);
            }
            self.current = None;
            RrTickAction::YieldToSamePriority
        } else {
            self.remaining_ticks = self.remaining_ticks.saturating_sub(1);
            RrTickAction::ContinueRunning
        }
    }

    /// 从所有桶移除任务；若为当前运行任务则清除时间片状态。
    pub fn remove(&mut self, task_id : TaskId) {
        if self.current.map(|(id, _)| id) == Some(task_id) {
            self.current = None;
            self.remaining_ticks = 0;
        }
        for bucket in &mut self.buckets {
            let _ = take_task_id_by_id(bucket, task_id);
        }
    }

    /// 任务解除阻塞后重新入队。
    pub fn on_task_unblocked(&mut self, task_id : TaskId, priority : i32) {
        self.enqueue(task_id, priority);
    }

    /// 当前 RR 任务是否应继续占用 CPU（时间片未耗尽）。
    pub fn should_continue_current(&self,
                                   check : &impl SchedulableCheck,
                                   current : TaskId,
                                   priority : i32)
                                   -> bool
    {
        self.current == Some((current, priority)) &&
            self.remaining_ticks > 0 &&
            check.is_schedulable(current)
    }

    /// 在指定优先级选取 RR 任务（含当前运行且时间片未尽的情况）。
    pub fn pick_at_priority(&mut self,
                            priority : i32,
                            check : &impl SchedulableCheck)
                            -> Option<TaskId>
    {
        if let Some((current, prio)) = self.current {
            if prio == priority && check.is_schedulable(current) && self.remaining_ticks > 0 {
                return Some(current);
            }
        }
        let index = bucket_index(priority)?;
        while let Some(task_id) = self.buckets[index].pop_front() {
            if check.is_schedulable(task_id) {
                self.current = Some((task_id, priority));
                self.remaining_ticks = MAX_RT_TICKS_PER_TASK;
                return Some(task_id);
            }
        }
        None
    }

    /// 记录任务成为当前 RR 运行者（从 FIFO/OTHER 切换过来时重置时间片）。
    pub fn note_running(&mut self, task_id : TaskId, priority : i32) {
        self.current = Some((task_id, priority));
        self.remaining_ticks = MAX_RT_TICKS_PER_TASK;
    }

    /// 清除当前 RR 运行状态（切换到非 RR 任务时）。
    pub fn clear_running(&mut self) {
        self.current = None;
        self.remaining_ticks = 0;
    }
}

fn take_task_id_by_id(queue : &mut VecDeque<TaskId>, task_id : TaskId) -> bool {
    let mut remaining = VecDeque::new();
    let mut found = false;
    while let Some(candidate) = queue.pop_front() {
        if candidate == task_id && !found {
            found = true;
        } else {
            remaining.push_back(candidate);
        }
    }
    *queue = remaining;
    found
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
            Self { live : ids.iter().copied().collect() }
        }
    }

    impl SchedulableCheck for MockCheck {
        fn is_schedulable(&self, task_id : TaskId) -> bool {
            self.live.contains(&task_id)
        }
    }

    #[test]
    fn higher_priority_picked_first() {
        let mut q = RtRrRunQueue::new();
        q.enqueue(10, 1);
        q.enqueue(20, 99);
        let check = MockCheck::new(&[10, 20]);
        assert_eq!(q.pick_next(&check), Some(20));
    }

    #[test]
    fn same_priority_round_robin() {
        let mut q = RtRrRunQueue::new();
        q.enqueue(1, 50);
        q.enqueue(2, 50);
        let check = MockCheck::new(&[1, 2]);
        assert_eq!(q.pick_next(&check), Some(1));
        for _ in 0..MAX_RT_TICKS_PER_TASK - 1 {
            assert_eq!(q.on_tick_current(1, 50), RrTickAction::ContinueRunning);
        }
        assert_eq!(q.on_tick_current(1, 50), RrTickAction::YieldToSamePriority);
        assert_eq!(q.pick_next(&check), Some(2));
    }
}
