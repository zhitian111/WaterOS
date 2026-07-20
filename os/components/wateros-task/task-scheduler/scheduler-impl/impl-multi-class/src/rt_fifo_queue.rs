//! 本模块代码由AI完成
//! `SCHED_FIFO` 就绪队列：99 个优先级桶，每桶 FIFO，无时间片。

extern crate alloc;

use alloc::collections::VecDeque;
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

/// `SCHED_FIFO` 就绪队列。
// 本结构代码由AI完成
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


    /// 从所有桶移除任务。
    pub fn remove(&mut self, task_id : TaskId) {
        for bucket in &mut self.buckets {
            let _ = take_task_id_by_id(bucket, task_id);
        }
    }


    /// 返回当前最高的可运行优先级，不改变队列内容。
    pub fn highest_runnable_priority(&self) -> Option<i32> {
        self.buckets
            .iter()
            .enumerate()
            .rev()
            .find(|(_, bucket)| !bucket.is_empty())
            .map(|(index, _)| (index as i32) + RT_PRIORITY_MIN)
    }

    /// 从指定优先级桶弹出首个可调度任务（供 multi-class 按优先级扫描）。
    pub fn pop_front_at_priority(&mut self, priority : i32) -> Option<TaskId> {
        let index = bucket_index(priority)?;
        self.buckets[index].pop_front()
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
