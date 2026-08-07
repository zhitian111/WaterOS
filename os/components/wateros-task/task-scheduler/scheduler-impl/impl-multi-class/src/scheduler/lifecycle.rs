// 任务终止、丢弃与回收。
use super::*;
impl MultiClassScheduler {
    pub fn kill_task(&mut self, task_id : TaskId, exit_code : TaskExitCode) -> bool {
        if self.registry.is_idle_task(task_id) {
            return false;
        }
        if self.registry
               .state(task_id)
               .is_none()
        {
            return false;
        }
        if matches!(self.registry
                        .state(task_id),
                    Some(TaskState::Exited(_)))
        {
            return true;
        }
        if self.cpu_states
               .iter()
               .any(|cpu| cpu.current_task_id() == Some(task_id))
        {
            return false;
        }
        self.dequeue_from_all_cpus(task_id);
        self.wait_queues
            .kill_task(task_id);
        self.registry
            .mark_exited(task_id, exit_code);
        true
    }

    pub fn discard_unstarted_task(&mut self, task_id : TaskId) {
        self.dequeue_from_all_cpus(task_id);
        self.wait_queues
            .detach_task_from_run_queues(task_id);
        self.registry
            .discard_task(task_id);
    }

    pub fn reap_exited_task(&mut self, task_id : TaskId) -> Option<ExitedTask> {
        self.wait_queues
            .reap_exited_task(&mut self.registry, task_id)
    }

    pub fn reap_one_exited_task(&mut self) -> Option<ExitedTask> {
        self.wait_queues
            .reap_one_exited_task(&mut self.registry)
    }

    pub fn reap_one_exited_child(&mut self, parent_id : TaskId) -> Option<ExitedTask> {
        let task_id = self.registry
                          .find_exited_child(parent_id)?;
        self.reap_exited_task(task_id)
    }
}
