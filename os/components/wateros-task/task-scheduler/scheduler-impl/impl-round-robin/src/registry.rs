extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use arch::task::ActiveArchTaskContext as TaskContext;
use task_api::{
    ExitedTask, KernelTaskEntry, TaskBlockReason, TaskExitCode, TaskId, TaskSnapshot, TaskState,
    TaskTick, TaskWaitHandle, TaskWaitResult, TaskWaitTarget, UserTaskSpec, IDLE_TASK_ID,
};
use task_impl::TaskControlBlock;

use crate::{SwitchPair, TaskTrapFrame};

unsafe extern "C" {
    safe fn __wateros_idle_task_runtime_main(arg: usize) -> !;
    fn __arch_idle_task_entry();
    fn __arch_task_entry();
    fn __arch_user_task_entry();
}

struct TaskTable {
    slots: Vec<Option<Box<TaskControlBlock>>>,
}

impl TaskTable {
    fn new() -> Self {
        Self { slots: Vec::new() }
    }

    fn clear(&mut self) {
        self.slots.clear();
    }

    fn insert(&mut self, task: Box<TaskControlBlock>) {
        let task_id = task.id();
        if self.slots.len() <= task_id {
            self.slots
                .resize_with(task_id + 1, || None);
        }
        assert!(
            self.slots[task_id].is_none(),
            "task slot {} already occupied",
            task_id
        );
        self.slots[task_id] = Some(task);
    }

    fn task(&self, task_id: TaskId) -> &TaskControlBlock {
        self.slots
            .get(task_id)
            .and_then(|slot| slot.as_deref())
            .expect("task must exist in task table")
    }

    fn task_mut(&mut self, task_id: TaskId) -> &mut TaskControlBlock {
        self.slots
            .get_mut(task_id)
            .and_then(|slot| slot.as_deref_mut())
            .expect("task must exist in task table")
    }

    fn remove(&mut self, task_id: TaskId) -> Option<Box<TaskControlBlock>> {
        self.slots
            .get_mut(task_id)
            .and_then(|slot| slot.take())
    }
}

pub(super) struct TaskRegistry {
    bootstrap_task_cx: TaskContext,
    task_table: TaskTable,
    current_task_id: Option<TaskId>,
    next_task_id: TaskId,
}

impl TaskRegistry {
    pub(super) fn new() -> Self {
        Self {
            bootstrap_task_cx: TaskContext::zero_init(),
            task_table: TaskTable::new(),
            current_task_id: None,
            next_task_id: IDLE_TASK_ID + 1,
        }
    }

    pub(super) fn init(&mut self) {
        self.bootstrap_task_cx = TaskContext::zero_init();
        self.task_table
            .clear();
        self.current_task_id = None;
        self.task_table
            .insert(Box::new(
                TaskControlBlock::new_idle_task(
                    IDLE_TASK_ID,
                    __arch_idle_task_entry as *const () as usize,
                    __wateros_idle_task_runtime_main,
                ),
            ));
        self.next_task_id = IDLE_TASK_ID + 1;
    }

    pub(super) fn spawn_kernel_task(&mut self, entry: KernelTaskEntry, arg: usize) -> TaskId {
        let task_id = self.next_task_id;
        self.next_task_id += 1;
        self.task_table
            .insert(Box::new(
                TaskControlBlock::new_kernel_task(
                    task_id,
                    __arch_task_entry as *const () as usize,
                    entry,
                    arg,
                ),
            ));
        task_id
    }

    pub(super) fn spawn_user_task_spec(&mut self, spec: UserTaskSpec) -> TaskId {
        let task_id = self.next_task_id;
        self.next_task_id += 1;
        self.task_table
            .insert(Box::new(
                TaskControlBlock::new_user_task(
                    task_id,
                    __arch_user_task_entry as *const () as usize,
                    spec,
                ),
            ));
        task_id
    }

    pub(super) fn first_switch_to(&mut self, next_task_id: TaskId) -> SwitchPair {
        let current_task_cx_ptr = &mut self.bootstrap_task_cx as *mut TaskContext;
        let next_task_cx_ptr = self.mark_running_and_set_current(next_task_id);
        (current_task_cx_ptr, next_task_cx_ptr)
    }

    pub(super) fn take_current_switch_out(&mut self) -> Option<(TaskId, *mut TaskContext)> {
        let current_task_id = self
            .current_task_id
            .take()?;
        let current_ptr = self
            .task_table
            .task_mut(current_task_id)
            .context_mut_ptr();
        Some((current_task_id, current_ptr))
    }

    pub(super) fn mark_running_and_set_current(&mut self, task_id: TaskId) -> *const TaskContext {
        self.task_table
            .task_mut(task_id)
            .mark_running();
        self.current_task_id = Some(task_id);
        self.task_table
            .task(task_id)
            .context_ptr()
    }

    pub(super) fn mark_ready(&mut self, task_id: TaskId) {
        self.task_table
            .task_mut(task_id)
            .mark_ready();
    }

    pub(super) fn mark_blocking(&mut self, task_id: TaskId, reason: TaskBlockReason) {
        self.task_table
            .task_mut(task_id)
            .mark_blocking(reason);
    }

    pub(super) fn mark_sleeping(&mut self, task_id: TaskId, wake_tick: TaskTick) {
        self.task_table
            .task_mut(task_id)
            .mark_sleeping(wake_tick);
    }

    pub(super) fn mark_exited(&mut self, task_id: TaskId, exit_code: TaskExitCode) {
        self.task_table
            .task_mut(task_id)
            .mark_exited(exit_code);
    }

    pub(super) fn ready_to_wake(&self, task_id: TaskId, current_tick: TaskTick) -> bool {
        self.task_table
            .task(task_id)
            .ready_to_wake(current_tick)
    }

    pub(super) fn is_idle(&self, task_id: TaskId) -> bool {
        self.task_table
            .task(task_id)
            .is_idle()
    }

    pub(super) fn state(&self, task_id: TaskId) -> Option<TaskState> {
        self.task_table
            .slots
            .get(task_id)
            .and_then(|slot| slot.as_deref())
            .map(TaskControlBlock::state)
    }

    pub(super) fn wait_target_ready(&self, wait_handle: TaskWaitHandle) -> bool {
        match wait_handle.target() {
            TaskWaitTarget::WaitQueue(_) => false,
            TaskWaitTarget::TaskExit(task_id) => self
                .state(task_id)
                .map(|state| matches!(state, TaskState::Exited(_)))
                .unwrap_or(true),
        }
    }

    pub(super) fn account_tick_for_current(&mut self) {
        if let Some(current_task_id) = self.current_task_id {
            if !self
                .task_table
                .task(current_task_id)
                .is_idle()
            {
                self.task_table
                    .task_mut(current_task_id)
                    .account_tick();
            }
        }
    }

    pub(super) fn current_task_id(&self) -> Option<TaskId> {
        self.current_task_id
    }

    pub(super) fn current_task_snapshot(&self) -> Option<TaskSnapshot> {
        self.current_task_id
            .map(|task_id| {
                self.task_table
                    .task(task_id)
                    .snapshot()
            })
    }

    pub(super) fn current_task_kernel_stack_top(&self) -> Option<usize> {
        self.current_task_id
            .map(|task_id| {
                self.task_table
                    .task(task_id)
                    .kernel_stack_top()
            })
    }

    pub(super) fn clear_wait_result(&mut self, task_id: TaskId) {
        self.task_table
            .task_mut(task_id)
            .clear_wait_result();
    }

    pub(super) fn finish_wait(&mut self, task_id: TaskId, result: TaskWaitResult) {
        self.task_table
            .task_mut(task_id)
            .finish_wait(result);
    }

    pub(super) fn take_current_wait_result(&mut self) -> TaskWaitResult {
        let current_task_id = self
            .current_task_id
            .expect("wait result can only be taken for a running task");
        self.task_table
            .task_mut(current_task_id)
            .take_wait_result()
    }

    pub(super) fn reap_task(&mut self, task_id: TaskId) -> Option<ExitedTask> {
        let task = self
            .task_table
            .remove(task_id)?;
        task.exited_task()
    }

    pub(super) fn record_current_trap_frame(&mut self, trap_frame: TaskTrapFrame) {
        if let Some(current_task_id) = self.current_task_id {
            self.task_table
                .task_mut(current_task_id)
                .record_trap_frame(trap_frame);
        }
    }

    pub(super) fn begin_current_trap_frame_access(
        &mut self,
        trap_frame: TaskTrapFrame,
    ) -> Option<*mut TaskTrapFrame> {
        let current_task_id = self.current_task_id?;
        Some(
            self.task_table
                .task_mut(current_task_id)
                .begin_trap_frame_access(trap_frame),
        )
    }

    pub(super) fn restore_current_trap_frame(&self, trap_frame: &mut TaskTrapFrame) -> bool {
        self.current_task_id
            .map(|current_task_id| {
                self.task_table
                    .task(current_task_id)
                    .restore_trap_frame_into(trap_frame)
            })
            .unwrap_or(false)
    }
}
