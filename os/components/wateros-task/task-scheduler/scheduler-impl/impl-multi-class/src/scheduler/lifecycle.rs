// 任务终止、丢弃与回收。
use super::*;
impl MultiClassScheduler {
    pub fn kill_task(&mut self, task_id : TaskId, exit_code : TaskExitCode) -> bool {
        if self.global
               .registry
               .is_idle(task_id)
        {
            return false;
        }
        if self.global
               .registry
               .state(task_id)
               .is_none()
        {
            return false;
        }
        if matches!(self.global
                        .registry
                        .state(task_id),
                    Some(TaskState::Exited(_)))
        {
            return true;
        }
        if self.cpu_states
               .iter()
               .any(|cpu| cpu.current_task_id == Some(task_id))
        {
            return false;
        }
        self.detach_from_all_cpus(task_id);
        self.global
            .wait_queues
            .kill_task(task_id);
        self.global
            .registry
            .mark_exited(task_id, exit_code);
        true
    }

    pub fn discard_unstarted_task(&mut self, task_id : TaskId) {
        self.detach_from_all_cpus(task_id);
        self.global
            .wait_queues
            .detach_task_from_run_queues(task_id);
        if self.global
               .registry
               .discard_task(task_id)
        {
            self.forget_task_on_all_cpus(task_id);
        }
    }

    pub fn reap_exited_task(&mut self, task_id : TaskId) -> Option<ExitedTask> {
        let exited = self.global
                         .wait_queues
                         .reap_exited_task(&mut self.global.registry, task_id)?;
        self.forget_task_on_all_cpus(task_id);
        Some(exited)
    }

    pub fn reap_one_exited_task(&mut self) -> Option<ExitedTask> {
        let exited = self.global
                         .wait_queues
                         .reap_one_exited_task(&mut self.global.registry)?;
        self.forget_task_on_all_cpus(exited.id);
        Some(exited)
    }

    pub fn reap_one_exited_child(&mut self, parent_id : TaskId) -> Option<ExitedTask> {
        let task_id = self.global
                          .registry
                          .find_exited_child(parent_id)?;
        self.reap_exited_task(task_id)
    }
}
