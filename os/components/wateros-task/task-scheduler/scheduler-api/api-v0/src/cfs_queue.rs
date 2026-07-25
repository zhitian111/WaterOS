//!cfs队列实现

use alloc::collections::{btree_map::BTreeMap, vec_deque::VecDeque};
use config::task::NICE_0_WEIGHT;
use config::task::NICE_TO_WEIGHT;
use core::iter::Take;
use task_api::{Nice, TaskId, VRunTime};

pub struct CfsQueue {
    tree : BTreeMap<VRunTime, VecDeque<TaskId>>,
    vruntime_for_task : BTreeMap<TaskId, VRunTime>,
}
impl CfsQueue {
    pub fn new() -> Self {
        Self { tree : BTreeMap::new(),
               vruntime_for_task : BTreeMap::new() }
    }
    //任务入队
    pub fn enqueue(&mut self, task_id : TaskId, vruntime : VRunTime) {
        self.vruntime_for_task
            .insert(task_id, vruntime);
        self.tree
            .entry(vruntime)
            .or_insert_with(VecDeque::new)
            .push_back(task_id);
    }
    //任务出队
    pub fn dequeue(&mut self, task_id : TaskId, vruntime : VRunTime) {
        self.vruntime_for_task
            .remove(&task_id);
        if let Some(tasks) = self.tree
                                 .get_mut(&vruntime)
        {
            tasks.retain(|id| *id != task_id);

            if tasks.is_empty() {
                self.tree
                    .remove(&vruntime); // 第二次查找
            }
        }
    }
    //选择下一个任务
    pub fn pick(&mut self) -> Option<TaskId> {
        self.tree
            .first_entry()
            .and_then(|mut entry| {
                entry.get_mut()
                     .pop_front()
            })
    }
    // 任务 tick 更新 vruntime，并返回是否需要抢占。
    pub fn tick(&mut self, task_id : TaskId, nice : Nice, vruntime : VRunTime) -> bool {
        let weight = NICE_TO_WEIGHT[(nice + 20) as usize];
        let delta = NICE_0_WEIGHT / weight;
        self.dequeue(task_id, vruntime);
        let cur_vruntime = vruntime + delta;
        self.enqueue(task_id, cur_vruntime);
        if let Some((first_vruntime, _)) = self.tree
                                               .first_key_value()
        {
            return *first_vruntime < cur_vruntime;
        }
        false
    }
}
