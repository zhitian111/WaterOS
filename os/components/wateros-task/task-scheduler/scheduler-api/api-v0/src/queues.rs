//! 多调度类的就绪队列。

extern crate alloc;

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};

use config::task::{MAX_RT_TICKS_PER_TASK, MAX_TICKS_PER_TASK};
use task_api::TaskId;
const RT_PRIORITY_MIN : i32 = 1;
const RT_PRIORITY_MAX : i32 = 99;
const RT_BUCKET_COUNT : usize = (RT_PRIORITY_MAX - RT_PRIORITY_MIN + 1) as usize;

/// `SCHED_OTHER` 任务的就绪队列。
///
/// 按 `(vruntime, task_id)` 排序，选择虚拟运行时间最小的任务。`entries`
/// 保留每个 task 当前的 key，因此防御性重复入队或跨 CPU 摘除都能精确移除
/// 旧条目，不需要 FIFO 版本号和 stale 条目清理。
pub struct OtherQueue {
    //按 (vruntime, task_id) 自动排序。调度器每次取最前面的元素
    ready_queue : BTreeSet<(u64, TaskId)>,
    // task_id -> vruntime 映射，便于精确移除旧条目
    entries : BTreeMap<TaskId, u64>,
    /// 本 CPU 已观察到的最小虚拟运行时间；只单调增加。
    min_vruntime : u64,
    current_ticks : u64,
}

impl OtherQueue {
    pub(super) fn new() -> Self {
        Self { ready_queue : BTreeSet::new(),
               entries : BTreeMap::new(),
               min_vruntime : 0,
               current_ticks : 0 }
    }

    /// Linux CFS 使用的 nice 权重（nice=-20..=19）。nice 越小，权重越大，
    /// 相同 wall-clock tick 下消耗的 vruntime 越少。
    const NICE_WEIGHTS : [u64; 40] = [88761, 71755, 56483, 46273, 36291, 29154, 23254, 18705,
                                      14949, 11916, 9548, 7620, 6100, 4904, 3906, 3121, 2501,
                                      1991, 1586, 1277, 1024, 820, 655, 526, 423, 335, 272, 215,
                                      172, 137, 110, 87, 70, 56, 45, 36, 29, 23, 18, 15];
    const NICE_0_WEIGHT : u64 = 1024;
    /// 将一个 scheduler tick 转换为 vruntime 的基础单位。
    const VRUNTIME_TICK_SCALE : u64 = Self::NICE_0_WEIGHT * Self::NICE_0_WEIGHT;

    /// 返回一个 scheduler tick 对 `nice` 任务增加的 vruntime。
    pub fn vruntime_delta(nice : i8) -> u64 {
        let index = (nice.clamp(-20, 19) + 20) as usize;
        (Self::VRUNTIME_TICK_SCALE / Self::NICE_WEIGHTS[index]).max(1)
    }

    /// 将新到达或迁入任务的 vruntime 向本 CPU 的时间线靠齐。
    ///
    /// 不降低已有 vruntime，避免刚迁移的 CPU 获得不应有的运行优势；也不允许
    /// 新建任务落在当前任务很久以前的时间线上并反复抢占。
    pub fn normalize_vruntime(&self, vruntime : u64) -> u64 { vruntime.max(self.min_vruntime) }

    /// 当前运行任务消耗时间后推进本 CPU 的虚拟时间线。
    ///
    /// 若队列内已有更久未运行的任务，`min_vruntime` 最多推进到该等待
    /// 任务的时间，不能直接跳到 current 的 vruntime。否则新唤醒任务会被
    /// 归一化到 current，同值时又由 task id 打破平局，可能长期落后于低
    /// task id 的周期性内核任务。
    pub fn observe_current_vruntime(&mut self, vruntime : u64) {
        let next_ready_vruntime = self.ready_queue
                                      .iter()
                                      .next()
                                      .map(|(ready_vruntime, _)| *ready_vruntime);
        let observed = next_ready_vruntime.map_or(vruntime,
                                                  |ready_vruntime| vruntime.min(ready_vruntime));
        self.min_vruntime = self.min_vruntime
                                .max(observed);
    }

    /// 记录当前任务消耗的 tick 数；返回是否已达到最大值。
    pub fn tick_current(&mut self) -> bool {
        self.current_ticks = self.current_ticks
                                 .saturating_add(1);
        self.current_ticks >= MAX_TICKS_PER_TASK
    }
    pub fn reset_ticks(&mut self) { self.current_ticks = 0; }
    /// 检查就绪队列中是否有可运行的任务。
    pub fn has_runnable(&self) -> bool {
        !self.ready_queue
             .is_empty()
    }

    /// 任务已从 registry 永久移除后从就绪队列回收。
    pub fn forget_task(&mut self, task_id : TaskId) { self.detach_task(task_id); }

    /// 将任务按其 vruntime 放入就绪队列。
    pub fn enqueue_ready_task(&mut self, task_id : TaskId, vruntime : u64) {
        self.detach_task(task_id);
        self.entries
            .insert(task_id, vruntime);
        self.ready_queue
            .insert((vruntime, task_id));
    }
    /// 清空就绪队列与虚拟时间线。
    pub fn init(&mut self) {
        self.ready_queue
            .clear();
        self.entries.clear();
        self.min_vruntime = 0;
        self.current_ticks = 0;
    }
    /// 从就绪队列精确摘除任务；任务不在本队列时为无操作。
    pub fn detach_task(&mut self, task_id : TaskId) {
        if let Some(vruntime) = self.entries
                                    .remove(&task_id)
        {
            self.ready_queue
                .remove(&(vruntime, task_id));
        }
    }
    /// 从就绪队列中选取下一个可运行任务号；若无则返回 `None`。
    pub fn pick_next_runnable_task_id(&mut self) -> Option<TaskId> {
        let (vruntime, task_id) = self.ready_queue
                                      .iter()
                                      .next()
                                      .copied()?;
        let removed = self.ready_queue
                          .remove(&(vruntime, task_id));
        assert!(removed,
                "selected OtherQueue entry must exist");
        let recorded = self.entries
                           .remove(&task_id);
        assert_eq!(recorded, Some(vruntime));
        self.min_vruntime = self.min_vruntime
                                .max(vruntime);
        Some(task_id)
    }
    pub fn runnable_count(&self) -> usize {
        self.ready_queue
            .len()
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


fn priority_from_index(index : usize) -> i32 { (index as i32) + RT_PRIORITY_MIN }

/// RR tick 处理结果。
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
