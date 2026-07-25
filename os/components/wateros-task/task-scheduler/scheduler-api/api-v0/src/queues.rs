//! `SCHED_OTHER` 专用就绪 FIFO。

extern crate alloc;

use alloc::collections::{BTreeMap, VecDeque};

use config::task::{MAX_TICKS_PER_TASK, READY_QUEUE_STALE_COMPACT_THRESHOLD};
use task_api::TaskId;
const RT_PRIORITY_MIN : i32 = 1;
const RT_PRIORITY_MAX : i32 = 99;
const RT_BUCKET_COUNT : usize = (RT_PRIORITY_MAX - RT_PRIORITY_MIN + 1) as usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct QueueEntry {
    task_id : TaskId,
    version : u64,
}

/// `SCHED_OTHER` 任务的就绪队列。
///
/// 使用 per-task 版本号实现 O(1) 入队与去重：重复入队只需在队尾追加新条目并使旧条目失效，
/// 避免原先 `VecDeque::retain` 在大量并存线程下退化为 O(n^2)。
pub struct OtherQueue {
    ready_queue : VecDeque<QueueEntry>,
    versions : BTreeMap<TaskId, u64>,
    current_ticks : u64,
}

impl OtherQueue {
    pub(super) fn new() -> Self {
        Self { ready_queue : VecDeque::new(),
               current_ticks : 0,
               versions : BTreeMap::new() }
    }
    /// 增加tick；返回是否已达到最大值。
    pub fn tick(&mut self) -> bool {
        self.current_ticks = self.current_ticks
                                 .saturating_add(1);
        self.current_ticks >= MAX_TICKS_PER_TASK
    }
    pub fn reset_ticks(&mut self) { self.current_ticks = 0; }
    /// 检查就绪队列条目是否仍然有效。
    fn entry_is_live(&self, entry : QueueEntry) -> bool {
        self.versions
            .get(&entry.task_id)
            .copied()
            .is_some_and(|ver| ver == entry.version)
    }
    /// 为指定任务号生成新的版本号并返回。
    fn bump_version(&mut self, task_id : TaskId) -> u64 {
        let entry = self.versions
                        .entry(task_id)
                        .or_insert(0);
        *entry = entry.saturating_add(1);
        *entry
    }
    /// 清理就绪队列中过期的条目。
    fn compact_stale_entries(&mut self) {
        let versions = &self.versions;
        self.ready_queue
            .retain(|entry| {
                versions.get(&entry.task_id)
                        .copied()
                        .is_some_and(|ver| ver == entry.version)
            });
    }
    /// 计算就绪队列中连续 stale 条目达到多少时触发清理。
    fn stale_compact_threshold(&self) -> usize {
        READY_QUEUE_STALE_COMPACT_THRESHOLD.max(self.ready_queue
                                                    .len() /
                                                4)
    }
    /// 检查就绪队列中是否有可运行的任务。
    pub fn has_runnable(&self) -> bool {
        self.ready_queue
            .iter()
            .copied()
            .any(|entry| self.entry_is_live(entry))
    }

    /// 任务已从 registry 永久移除后回收 `versions` 条目。
    pub fn forget_task(&mut self, task_id : TaskId) {
        self.versions
            .remove(&task_id);
    }
    /// 将任务入就绪队列尾部；若已存在则生成新版本号并使旧条目失效。
    pub fn enqueue_ready_task(&mut self, task_id : TaskId) {
        let version = self.bump_version(task_id);
        self.ready_queue
            .push_back(QueueEntry { task_id, version });
    }
    /// 清空就绪队列与版本号表。
    pub fn init(&mut self) {
        self.ready_queue
            .clear();
        self.versions
            .clear();
        self.current_ticks = 0;
    }
    /// 任务被调度运行后从就绪队列中移除；若已存在多个条目则只使其余条目失效。
    pub fn detach_task(&mut self, task_id : TaskId) { let _ = self.bump_version(task_id); }
    /// 从就绪队列中选取下一个可运行任务号；若无则返回 `None`。
    pub fn pick_next_runnable_task_id(&mut self) -> Option<TaskId> {
        let mut consecutive_stale = 0usize;
        while let Some(entry) = self.ready_queue
                                    .pop_front()
        {
            if !self.entry_is_live(entry) {
                consecutive_stale = consecutive_stale.saturating_add(1);
                if consecutive_stale >= self.stale_compact_threshold() {
                    self.compact_stale_entries();
                    consecutive_stale = 0;
                }
                continue;
            }
            return Some(entry.task_id);
        }
        None
    }
    pub fn runnable_count(&self) -> usize {
        self.ready_queue
            .iter()
            .copied()
            .filter(|entry| self.entry_is_live(*entry))
            .count()
    }
}


fn bucket_index(priority : i32) -> Option<usize> {
    if (RT_PRIORITY_MIN..=RT_PRIORITY_MAX).contains(&priority) {
        Some((priority - RT_PRIORITY_MIN) as usize)
    } else {
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
/// `SCHED_FIFO` 就绪队列。
pub struct FifoQueue {
    buckets : [VecDeque<TaskId>; RT_BUCKET_COUNT],
}

impl FifoQueue {
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
    pub fn runnable_count(&self) -> usize {
        self.buckets
            .iter()
            .map(|bucket| bucket.len())
            .sum()
    }
}


use config::task::MAX_RT_TICKS_PER_TASK;


fn priority_from_index(index : usize) -> i32 { (index as i32) + RT_PRIORITY_MIN }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RrTickAction {
    /// 继续运行当前 RR 任务。
    ContinueRunning,
    /// 时间片耗尽，让出给同优先级下一个 RR 任务。
    YieldToSamePriority,
}

/// `SCHED_RR` 就绪队列。
pub struct RrQueue {
    buckets : [VecDeque<TaskId>; RT_BUCKET_COUNT],
    current : Option<(TaskId, i32)>,
    remaining_ticks : u64,
}

impl RrQueue {
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


    /// 处理当前 RR 任务的 tick 消耗；返回是否应让出 CPU。
    pub fn tick(&mut self, current : TaskId, priority : i32) -> bool {
        if self.current != Some((current, priority)) {
            self.current = Some((current, priority));
            self.remaining_ticks = MAX_RT_TICKS_PER_TASK;
        }
        if self.remaining_ticks <= 1 {
            self.remaining_ticks = 0;
            self.current = None;
            true
        } else {
            self.remaining_ticks = self.remaining_ticks
                                       .saturating_sub(1);
            false
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
            .find(|(_, bucket)| !bucket.is_empty())
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
    pub fn runnable_count(&self) -> usize {
        self.buckets
            .iter()
            .map(|bucket| bucket.len())
            .sum()
    }
}
