//! 本模块代码由AI完成
//! `SCHED_RR` 就绪队列：优先级桶 + 同优先级时间片轮转。

extern crate alloc;

use alloc::collections::VecDeque;
use config::task::MAX_RT_TICKS_PER_TASK;
use task_api::TaskId;

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

fn priority_from_index(index : usize) -> i32 { (index as i32) + RT_PRIORITY_MIN }

/// RR tick 处理结果。
// 本结构代码由AI完成
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RrTickAction {
    /// 继续运行当前 RR 任务。
    ContinueRunning,
    /// 时间片耗尽，让出给同优先级下一个 RR 任务。
    YieldToSamePriority,
}

/// `SCHED_RR` 就绪队列。
// 本结构代码由AI完成
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


    /// 时钟 tick：处理当前 RR 任务时间片。
    pub fn on_tick_current(&mut self, current : TaskId, priority : i32) -> RrTickAction {
        if self.current != Some((current, priority)) {
            self.current = Some((current, priority));
            self.remaining_ticks = MAX_RT_TICKS_PER_TASK;
        }
        if self.remaining_ticks <= 1 {
            self.remaining_ticks = 0;
            self.current = None;
            RrTickAction::YieldToSamePriority
        } else {
            self.remaining_ticks = self.remaining_ticks
                                       .saturating_sub(1);
            RrTickAction::ContinueRunning
        }
    }

    /// 从所有桶移除任务；若为当前运行任务则清除时间片状态。
    pub fn remove(&mut self, task_id : TaskId) {
        if self.current
               .map(|(id, _)| id) ==
           Some(task_id)
        {
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

    /// 返回就绪桶中最高的可运行优先级，不包含当前运行任务。
    pub fn highest_ready_priority(&self) -> Option<i32> {
        self.buckets
            .iter()
            .enumerate()
            .rev()
            .find(|(_, bucket)| {
                !bucket.is_empty()
            })
            .map(|(index, _)| priority_from_index(index))
    }

    /// 当前 RR 任务是否应继续占用 CPU（时间片未耗尽）。
    pub fn should_continue_current(&self, current : TaskId, priority : i32) -> bool {
        self.current == Some((current, priority)) && self.remaining_ticks > 0
    }

    /// 在指定优先级选取 RR 任务（含当前运行且时间片未尽的情况）。
    pub fn pick_at_priority(&mut self, priority : i32) -> Option<TaskId> {
        if let Some((current, prio)) = self.current {
            if prio == priority && self.remaining_ticks > 0 {
                return Some(current);
            }
        }
        let index = bucket_index(priority)?;
        while let Some(task_id) = self.buckets[index].pop_front() {
            self.current = Some((task_id, priority));
            self.remaining_ticks = MAX_RT_TICKS_PER_TASK;
            return Some(task_id);
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

    /// 按优先级从高到低选取下一个任务。
    pub fn pick_next(&mut self) -> Option<TaskId> {
        if let Some((current, prio)) = self.current {
            if self.remaining_ticks > 0 {
                return Some(current);
            }
        }
        for priority in (RT_PRIORITY_MIN..=RT_PRIORITY_MAX).rev() {
            if let Some(task_id) = self.pick_at_priority(priority) {
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

    #[test]
    fn higher_priority_picked_first() {
        let mut q = RtRrRunQueue::new();
        q.enqueue(10, 1);
        q.enqueue(20, 99);
        assert_eq!(q.pick_next(), Some(20));
    }

    #[test]
    fn same_priority_round_robin() {
        let mut q = RtRrRunQueue::new();
        q.enqueue(1, 50);
        q.enqueue(2, 50);
        assert_eq!(q.pick_next(), Some(1));
        for _ in 0..MAX_RT_TICKS_PER_TASK - 1 {
            assert_eq!(q.on_tick_current(1, 50),
                       RrTickAction::ContinueRunning);
        }
        assert_eq!(q.on_tick_current(1, 50),
                   RrTickAction::YieldToSamePriority);
        assert_eq!(q.pick_next(), Some(2));
    }

    #[test]
    fn highest_priority_returned() {
        let mut q = RtRrRunQueue::new();
        q.enqueue(1, 90);
        q.enqueue(2, 40);
        assert_eq!(q.highest_ready_priority(),
                   Some(90));
    }
}
