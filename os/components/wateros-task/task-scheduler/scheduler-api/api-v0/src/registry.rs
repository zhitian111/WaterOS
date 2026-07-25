//! **任务注册表**：可复用槽位 + generation 编码的 `TaskId` → `TaskControlBlock`
//! 表、当前运行任务指针，以及首次切换用的引导上下文。

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use arch::task::ActiveArchTaskContext as TaskContext;
use arch::trap::ActiveTrapFrame as TaskTrapFrame;
use task_api::{
    CpuId, CpuMask, ExitedTask, KernelTaskEntry, SchedError, SchedPolicy, TaskExitCode, TaskId,
    TaskSnapshot, TaskState, TaskTick, TaskWaitResult, TaskWaitTarget, UserTask, IDLE_TASK_ID,
};
use task_impl::TaskControlBlock;


unsafe extern "C" {
    /// 聚合 crate 提供的 idle 循环体；idle TCB 的入口地址取自此符号。
    safe fn __wateros_idle_task_runtime_main(arg : usize) -> !;
}

/// 各调度器实现共享的 TCB 表与「当前任务」元数据。
pub struct TaskRegistry {
    tasks : BTreeMap<TaskId, Box<TaskControlBlock>>,
    next_id : TaskId,
}

impl TaskRegistry {
    /// 构造空注册表（须再调用 [`Self::init`]）。
    pub fn new() -> Self {
        Self { tasks : BTreeMap::new(),
               next_id : 0 }
    }

    /// 重置并插入 idle 任务。
    pub fn init(&mut self) {
        self.tasks.clear();
        self.next_id = 0;
    }

    /// 创建一个 idle 任务，返回其 task_id。
    /// 每个 CPU 应调用一次以创建其专属 idle TCB。
    pub fn spawn_idle_task(&mut self) -> TaskId {
        let task_id = {
            let id = self.next_id;
            self.next_id = self.next_id
                               .saturating_add(1);
            id
        };
        self.tasks
            .insert(task_id,
                    Box::new(TaskControlBlock::new_idle_task(task_id,
                                                             __wateros_idle_task_runtime_main)));
        task_id
    }
    /// 创建内核任务并返回其 id。
    pub fn spawn_kernel_task(&mut self,
                             entry : KernelTaskEntry,
                             arg : usize,
                             parent_id : Option<TaskId>)
                             -> TaskId {
        let task_id = {
            let id = self.next_id;
            self.next_id = self.next_id
                               .saturating_add(1);
            id
        };
        self.tasks
            .insert(task_id,
                    Box::new(TaskControlBlock::new_kernel_task(task_id, parent_id, entry, arg)));
        task_id
    }

    /// 按规格创建用户任务并返回其 id。
    pub fn spawn_user_task_spec(&mut self, spec : UserTask, parent_id : Option<TaskId>) -> TaskId {
        let task_id = {
            let id = self.next_id;
            self.next_id = self.next_id
                               .saturating_add(1);
            id
        };
        log::trace!("[task-spawn] user spec id={} entry_pc={:#x} address_space_raw={:#x} \
                     image={:?} external_stack={:?}",
                    task_id,
                    spec.entry_pc(),
                    spec.address_space()
                        .map(|h| h.raw())
                        .unwrap_or(0),
                    spec.image(),
                    spec.stack());
        self.tasks
            .insert(task_id,
                    Box::new(TaskControlBlock::new_user_task(task_id, parent_id, spec)));
        task_id
    }

    /// 从当前任务 fork 子用户任务。
    pub fn fork_current(&mut self,
                        child_stack : usize,
                        new_aspace_ptr : usize,
                        new_satp : usize,
                        parent_id : TaskId)
                        -> Option<TaskId> {
        let child_id = {
            let id = self.next_id;
            self.next_id = self.next_id
                               .saturating_add(1);
            id
        };
        let parent = self.tasks
                         .get(&parent_id)
                         .map(|b| b.as_ref())
                         .expect("parent task must exist");
        let child = match parent.fork_from(child_id,
                                           child_stack,
                                           new_aspace_ptr,
                                           new_satp)
        {
            Some(child) => child,
            None => return None,
        };
        self.tasks
            .insert(child.id(), Box::new(child));
        Some(child_id)
    }

    /// 从当前用户任务 clone 同进程线程。
    pub fn clone_current_thread(&mut self,
                                child_stack : usize,
                                tls : usize,
                                set_tls : bool,
                                parent_id : TaskId)
                                -> Option<TaskId> {
        let child_id = {
            let id = self.next_id;
            self.next_id = self.next_id
                               .saturating_add(1);
            id
        };
        let parent = self.tasks
                         .get(&parent_id)
                         .map(|b| b.as_ref())
                         .expect("parent task must exist");
        let child = match parent.clone_thread_from(child_id, child_stack, tls, set_tls) {
            Some(child) => child,
            None => return None,
        };
        self.tasks
            .insert(child.id(), Box::new(child));
        Some(child_id)
    }

    /// execve：替换当前任务的地址空间、入口和栈。
    pub fn execve_current(&mut self,
                          entry_pc : usize,
                          sp : usize,
                          argc : usize,
                          argv : usize,
                          envp : usize,
                          satp : usize,
                          user_aspace_ptr : usize,
                          image_info : task_api::UserImageInfo,
                          stack_info : task_api::UserStack,
                          current_id : TaskId) {
        self.tasks
            .get_mut(&current_id)
            .map(|b| b.as_mut())
            .expect("task must exist")
            .execve_from(entry_pc,
                         sp,
                         argc,
                         argv,
                         envp,
                         satp,
                         user_aspace_ptr,
                         image_info,
                         stack_info);
    }

    pub fn take_task_cx(&mut self, task_id : TaskId) -> *mut TaskContext {
        self.tasks
            .get_mut(&task_id)
            .map(|b| b.as_mut())
            .expect("task must exist")
            .context_mut_ptr()
    }

    pub fn task_cx_ptr(&self, task_id : TaskId) -> *const TaskContext {
        self.tasks
            .get(&task_id)
            .map(|b| b.as_ref())
            .expect("task must exist")
            .context_ptr()
    }

    pub fn tick(&mut self, task_id : TaskId) {
        if let Some(task) = self.tasks
                                .get_mut(&task_id)
                                .map(|b| b.as_mut())
        {
            if !task.is_idle() {
                task.tick();
            }
        }
    }

    /// 将 `task_id` 标为 Running 并设为当前任务，返回其只读上下文指针。
    pub fn mark_running(&mut self, task_id : TaskId, cpu_id : CpuId) {
        self.tasks
            .get_mut(&task_id)
            .map(|b| b.as_mut())
            .expect("task must exist")
            .mark_running(cpu_id);
    }

    /// 更新任务的调度策略与优先级（仅 TCB 字段，不迁移 run-queue）。
    pub fn set_task_sched(&mut self,
                          task_id : TaskId,
                          policy : SchedPolicy,
                          priority : i32)
                          -> bool {
        if let Some(task) = self.tasks
                                .get_mut(&task_id)
                                .map(|b| b.as_mut())
        {
            task.set_sched(policy, priority);
            true
        } else {
            false
        }
    }

    /// 将任务标为 Ready。
    pub fn mark_ready(&mut self, task_id : TaskId, cpu_id : CpuId) {
        if let Some(task) = self.tasks
                                .get_mut(&task_id)
                                .map(|b| b.as_mut())
        {
            task.mark_ready(cpu_id);
        }
    }

    /// 将任务标为 Blocking 并记录原因。
    pub fn mark_blocking(&mut self, task_id : TaskId, reason : TaskWaitTarget) {
        if let Some(task) = self.tasks
                                .get_mut(&task_id)
                                .map(|b| b.as_mut())
        {
            task.mark_blocking(reason);
        }
    }

    /// 将任务标为 Sleeping 并设置唤醒 tick。
    pub fn mark_sleeping(&mut self, task_id : TaskId, wake_tick : TaskTick) {
        if let Some(task) = self.tasks
                                .get_mut(&task_id)
                                .map(|b| b.as_mut())
        {
            task.mark_sleeping(wake_tick);
        }
    }

    /// 将任务标为 Exited。
    pub fn mark_exited(&mut self, task_id : TaskId, exit_code : TaskExitCode) {
        if let Some(task) = self.tasks
                                .get_mut(&task_id)
                                .map(|b| b.as_mut())
        {
            task.mark_exited(exit_code);
        }
    }

    pub fn ready_to_wake(&self, task_id : TaskId, current_tick : TaskTick) -> bool {
        self.tasks
            .get(&task_id)
            .map(|b| b.as_ref())
            .is_some_and(|task| task.ready_to_wake(current_tick))
    }

    pub fn is_idle(&self, task_id : TaskId) -> bool {
        self.tasks
            .get(&task_id)
            .map(|b| b.as_ref())
            .is_some_and(TaskControlBlock::is_idle)
    }

    pub fn state(&self, task_id : TaskId) -> Option<TaskState> {
        self.tasks
            .get(&task_id)
            .map(|b| b.as_ref())
            .map(TaskControlBlock::state)
    }

    pub fn ready_cpu_id(&self, task_id : TaskId) -> Option<CpuId> {
        self.tasks
            .get(&task_id)
            .map(|b| b.as_ref())
            .and_then(TaskControlBlock::ready_cpu_id)
    }

    pub fn last_cpu_id(&self, task_id : TaskId) -> Option<CpuId> {
        self.tasks
            .get(&task_id)
            .map(|b| b.as_ref())
            .and_then(TaskControlBlock::last_cpu_id)
    }

    pub fn running_cpu_id(&self, task_id : TaskId) -> Option<CpuId> {
        self.tasks
            .get(&task_id)
            .map(|b| b.as_ref())
            .and_then(TaskControlBlock::running_cpu_id)
    }

    pub fn wait_target_ready(&self, target : TaskWaitTarget) -> bool {
        match target {
            TaskWaitTarget::WaitQueue(_) => false,
            TaskWaitTarget::TaskExit(task_id) => {
                self.state(task_id)
                    .map(|state| matches!(state, TaskState::Exited(_)))
                    .unwrap_or(true)
            }
            TaskWaitTarget::ChildExit(parent_id) => {
                self.find_exited_child(parent_id)
                    .is_some() ||
                !self.has_child(parent_id)
            }
            TaskWaitTarget::Manual => false,
        }
    }

    pub fn parent_id(&self, task_id : TaskId) -> Option<TaskId> {
        self.tasks
            .get(&task_id)
            .map(|b| b.as_ref())
            .and_then(TaskControlBlock::parent_id)
    }

    pub fn has_child(&self, parent_id : TaskId) -> bool {
        self.tasks
            .values()
            .map(|b| b.as_ref())
            .any(|task| task.parent_id() == Some(parent_id))
    }

    pub fn find_exited_child(&self, parent_id : TaskId) -> Option<TaskId> {
        self.tasks
            .values()
            .map(|b| b.as_ref())
            .find(|task| {
                task.parent_id() == Some(parent_id) && matches!(task.state(), TaskState::Exited(_))
            })
            .map(TaskControlBlock::id)
    }


    pub fn task_snapshot(&self, task_id : TaskId) -> TaskSnapshot {
        self.tasks
            .get(&task_id)
            .map(|b| b.as_ref())
            .expect("task must exist")
            .snapshot()
    }

    pub fn task_kernel_stack_top(&self, task_id : TaskId) -> usize {
        self.tasks
            .get(&task_id)
            .map(|b| b.as_ref())
            .expect("task must exist")
            .kernel_stack_top()
    }

    pub fn current_task_address_space_raw(&self, task_id : TaskId) -> usize {
        self.tasks
            .get(&task_id)
            .map(|b| b.as_ref())
            .expect("task must exist")
            .user_address_space_raw()
    }

    pub fn current_task_user_aspace_ptr(&self, task_id : TaskId) -> usize {
        self.tasks
            .get(&task_id)
            .map(|b| b.as_ref())
            .expect("task must exist")
            .user_aspace_ptr()
    }

    pub fn current_task_trap_return_address_space_token(&self, task_id : TaskId) -> usize {
        self.tasks
            .get(&task_id)
            .map(|b| b.as_ref())
            .expect("task must exist")
            .trap_return_address_space_token()
    }

    pub fn clear_wait_result(&mut self, task_id : TaskId) {
        if let Some(task) = self.tasks
                                .get_mut(&task_id)
                                .map(|b| b.as_mut())
        {
            task.clear_wait_result();
        }
    }

    pub fn finish_wait(&mut self, task_id : TaskId, result : TaskWaitResult) {
        if let Some(task) = self.tasks
                                .get_mut(&task_id)
                                .map(|b| b.as_mut())
        {
            task.finish_wait(result);
        }
    }

    pub fn take_current_wait_result(&mut self, task_id : TaskId) -> TaskWaitResult {
        self.tasks
            .get_mut(&task_id)
            .map(|b| b.as_mut())
            .expect("task must exist")
            .take_wait_result()
    }

    pub fn reap_task(&mut self, task_id : TaskId) -> Option<ExitedTask> {
        let task = self.tasks
                       .remove(&task_id)?;
        task.exited_task()
    }

    /// 丢弃尚未运行或刚创建、尚未进入 Exited 状态的任务（fork/clone 失败回滚）。
    pub fn discard_task(&mut self, task_id : TaskId) -> bool {
        self.tasks
            .remove(&task_id)
            .is_some()
    }

    pub fn begin_trap_frame_access(&mut self,
                                   trap_frame : TaskTrapFrame,
                                   task_id : TaskId)
                                   -> Option<*mut TaskTrapFrame> {
        if self.is_idle(task_id) {
            return None;
        }
        if !self.tasks
                .get(&task_id)
                .map(|b| b.as_ref())
                .expect("task must exist")
                .is_user()
        {
            return None;
        }
        Some(self.tasks
                 .get_mut(&task_id)
                 .map(|b| b.as_mut())
                 .expect("task must exist")
                 .begin_trap_frame_access(trap_frame))
    }

    pub fn restore_trap_frame(&self, trap_frame : &mut TaskTrapFrame, task_id : TaskId) -> bool {
        self.tasks
            .get(&task_id)
            .map(|b| b.as_ref())
            .expect("task must exist")
            .restore_trap_frame_into(trap_frame)
    }
    pub fn is_schedulable(&self, task_id : TaskId) -> bool {
        if task_id == IDLE_TASK_ID {
            return self.tasks
                       .get(&task_id)
                       .map(|b| b.as_ref())
                       .is_some();
        }
        self.tasks
            .get(&task_id)
            .map(|b| b.as_ref())
            .is_some_and(|task| matches!(task.state(), TaskState::Ready))
    }
    pub fn set_affinity(&mut self, task_id : TaskId, mask : CpuMask) -> Result<(), SchedError> {
        if let Some(task) = self.tasks
                                .get_mut(&task_id)
                                .map(|b| b.as_mut())
        {
            task.set_affinity(mask);
            Ok(())
        } else {
            Err(SchedError::NoSuchTask)
        }
    }
    pub fn get_affinity(&self, task_id : TaskId) -> Result<CpuMask, SchedError> {
        if let Some(task) = self.tasks
                                .get(&task_id)
                                .map(|b| b.as_ref())
        {
            Ok(task.affinity())
        } else {
            Err(SchedError::NoSuchTask)
        }
    }

    /// 更新 task-level nice 属性；runqueue 位置由 scheduler 层决定。
    pub fn set_nice(&mut self, task_id : TaskId, nice : i8) -> Result<(), SchedError> {
        if let Some(task) = self.tasks
                                .get_mut(&task_id)
                                .map(|b| b.as_mut())
        {
            task.set_nice(nice);
            Ok(())
        } else {
            Err(SchedError::NoSuchTask)
        }
    }

    pub fn get_nice(&self, task_id : TaskId) -> Result<i8, SchedError> {
        self.tasks
            .get(&task_id)
            .map(|b| b.as_ref())
            .map(|task| task.nice())
            .ok_or(SchedError::NoSuchTask)
    }
    pub fn policy(&self, task_id : TaskId) -> Result<SchedPolicy, SchedError> {
        self.tasks
            .get(&task_id)
            .map(|b| b.as_ref())
            .map(|task| task.policy())
            .ok_or(SchedError::NoSuchTask)
    }
    pub fn priority(&self, task_id : TaskId) -> Result<i32, SchedError> {
        self.tasks
            .get(&task_id)
            .map(|b| b.as_ref())
            .map(|task| task.priority())
            .ok_or(SchedError::NoSuchTask)
    }
}
