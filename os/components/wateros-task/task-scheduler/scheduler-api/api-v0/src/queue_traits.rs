//! 调度实现共享的就绪队列契约。

extern crate alloc;

use alloc::collections::VecDeque;
use task_api::{SchedulableCheck, TaskId};

/// 可接收“重新变为就绪”的任务。
pub trait ReadyTaskSink {
    /// 将任务放入实现选择的就绪路径。
    fn enqueue_ready_task(&mut self, task_id: TaskId);

    /// 从就绪路径中移除指定任务；不支持反向移除的临时 staging sink 可保持默认空操作。
    fn detach_ready_task(&mut self, _task_id: TaskId) {}
}

impl ReadyTaskSink for VecDeque<TaskId> {
    fn enqueue_ready_task(&mut self, task_id: TaskId) {
        self.retain(|candidate_task_id| *candidate_task_id != task_id);
        self.push_back(task_id);
    }

    fn detach_ready_task(&mut self, task_id: TaskId) {
        self.retain(|candidate_task_id| *candidate_task_id != task_id);
    }
}

/// 具体调度类的就绪队列。
pub trait ReadyQueue: ReadyTaskSink {
    /// 清空队列。
    fn init(&mut self);
    /// 从队列中移除指定任务。
    fn detach_task(&mut self, task_id: TaskId);
    /// 选出下一个可运行任务；无任务时返回 `None`。
    fn pick_next_runnable_task_id(&mut self, check: &impl SchedulableCheck) -> Option<TaskId>;
}
