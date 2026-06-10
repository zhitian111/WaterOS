//! `SCHED_OTHER` 专用就绪 FIFO。

extern crate alloc;

use alloc::collections::VecDeque;
use task_api::{SchedulableCheck, TaskId, IDLE_TASK_ID};

use crate::TaskRegistry;

/// `SCHED_OTHER` 任务的就绪队列。
pub struct OtherReadyQueue {
    ready_queue : VecDeque<TaskId>,
}

impl OtherReadyQueue {
    /// 构造空队列。
    pub fn new() -> Self {
        Self { ready_queue : VecDeque::new() }
    }

    /// 清空队列。
    pub fn init(&mut self) {
        self.ready_queue
            .clear();
    }

    /// 将新 spawn 的 OTHER 任务入队尾。
    pub fn push_spawned_task(&mut self, task_id : TaskId) {
        self.ready_queue
            .push_back(task_id);
    }

    /// 可变访问底层 FIFO（供 `WaitQueues` 暂存唤醒任务时批量路由）。
    pub fn ready_queue_mut(&mut self) -> &mut VecDeque<TaskId> {
        &mut self.ready_queue
    }

    /// 从就绪队列弹出第一个存在且未退出的任务；跳过 stale 项。
    pub fn pick_next_runnable_task_id(&mut self, registry : &TaskRegistry) -> TaskId {
        while let Some(task_id) = self.ready_queue.pop_front() {
            if registry.is_schedulable(task_id) {
                return task_id;
            }
            log::trace!("[task-scheduler] skip unrunnable task {} in other ready_queue",
                        task_id);
        }
        IDLE_TASK_ID
    }

    /// 将任务从 OTHER 就绪队列移除。
    pub fn detach_task(&mut self, task_id : TaskId) {
        let _ = take_task_id_by_id(&mut self.ready_queue, task_id);
    }
}

fn take_task_id_by_id(queue : &mut VecDeque<TaskId>, task_id : TaskId) -> bool {
    let mut remaining = VecDeque::new();
    let mut found = false;
    while let Some(candidate_task_id) = queue.pop_front() {
        if candidate_task_id == task_id && !found {
            found = true;
        } else {
            remaining.push_back(candidate_task_id);
        }
    }
    *queue = remaining;
    found
}
