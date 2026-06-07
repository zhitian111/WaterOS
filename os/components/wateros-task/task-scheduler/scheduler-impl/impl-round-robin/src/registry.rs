//! **任务注册表**：可复用槽位 + generation 编码的 `TaskId` → `TaskControlBlock`
//! 表、当前运行任务指针，以及首次切换用的引导上下文。

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use arch::task::{ActiveArchTaskContext as TaskContext, ArchTaskContext};
use task_api::{
    ExitedTask, KernelTaskEntry, TaskBlockReason, TaskExitCode, TaskId, TaskSnapshot, TaskState,
    TaskTick, TaskWaitHandle, TaskWaitResult, TaskWaitTarget, UserTask, IDLE_TASK_ID,
};
use task_impl::TaskControlBlock;

use crate::{SwitchPair, TaskTrapFrame};

unsafe extern "C" {
    /// 聚合 crate 提供的 idle 循环体；idle TCB 的入口地址取自此符号。
    safe fn __wateros_idle_task_runtime_main(arg : usize) -> !;
}

const TASK_ID_SLOT_BITS : usize = 32;
const TASK_ID_SLOT_MASK : usize = (1usize << TASK_ID_SLOT_BITS) - 1;

#[inline]
fn task_slot(task_id : TaskId) -> usize { task_id & TASK_ID_SLOT_MASK }

#[inline]
fn task_generation(task_id : TaskId) -> usize { task_id >> TASK_ID_SLOT_BITS }

#[inline]
fn make_task_id(slot : usize, generation : usize) -> TaskId {
    (generation << TASK_ID_SLOT_BITS) | slot
}

struct TaskSlot {
    generation : usize,
    task : Option<Box<TaskControlBlock>>,
}

impl TaskSlot {
    fn empty(generation : usize) -> Self {
        Self { generation,
               task : None }
    }
}

// `TaskId` 低位为槽号，高位为 generation。reap 后槽位进入 free-list；
// 下一次复用时 generation 已增加，因此旧 TaskId 无法命中新任务。
struct TaskTable {
    slots : Vec<TaskSlot>,
    free_slots : Vec<usize>,
}

impl TaskTable {
    fn new() -> Self {
        Self { slots : Vec::new(),
               free_slots : Vec::new() }
    }

    fn clear(&mut self) {
        self.slots.clear();
        self.free_slots.clear();
    }

    fn allocate_id(&mut self) -> TaskId {
        if let Some(slot) = self.free_slots.pop() {
            let generation = self.slots[slot].generation;
            return make_task_id(slot, generation);
        }
        let slot = self.slots.len();
        self.slots.push(TaskSlot::empty(0));
        make_task_id(slot, 0)
    }

    fn insert(&mut self, task : Box<TaskControlBlock>) {
        let task_id = task.id();
        let slot = task_slot(task_id);
        let generation = task_generation(task_id);
        if self.slots.len() <= slot {
            self.slots
                .resize_with(slot + 1, || TaskSlot::empty(0));
        }
        assert!(self.slots[slot].generation == generation,
                "task slot {} generation mismatch for id {}",
                slot,
                task_id);
        assert!(self.slots[slot].task.is_none(),
                "task slot {} already occupied",
                slot);
        self.slots[slot].task = Some(task);
    }

    fn task(&self, task_id : TaskId) -> &TaskControlBlock {
        self.task_opt(task_id)
            .expect("task must exist in task table")
    }

    fn task_mut(&mut self, task_id : TaskId) -> &mut TaskControlBlock {
        self.task_mut_opt(task_id)
            .expect("task must exist in task table")
    }

    fn task_mut_opt(&mut self, task_id : TaskId) -> Option<&mut TaskControlBlock> {
        let slot = task_slot(task_id);
        let generation = task_generation(task_id);
        self.slots
            .get_mut(slot)
            .filter(|entry| entry.generation == generation)
            .and_then(|entry| entry.task.as_deref_mut())
    }

    fn task_opt(&self, task_id : TaskId) -> Option<&TaskControlBlock> {
        let slot = task_slot(task_id);
        let generation = task_generation(task_id);
        self.slots
            .get(slot)
            .filter(|entry| entry.generation == generation)
            .and_then(|entry| entry.task.as_deref())
    }

    fn remove(&mut self, task_id : TaskId) -> Option<Box<TaskControlBlock>> {
        let slot = task_slot(task_id);
        let generation = task_generation(task_id);
        let entry = self.slots
                        .get_mut(slot)?;
        if entry.generation != generation {
            return None;
        }
        let task = entry.task.take()?;
        if slot != task_slot(IDLE_TASK_ID) {
            entry.generation = entry.generation.saturating_add(1);
            self.free_slots.push(slot);
        }
        Some(task)
    }

    fn iter_tasks(&self) -> impl Iterator<Item = &TaskControlBlock> {
        self.slots.iter().filter_map(|slot| slot.task.as_deref())
    }
}

/// 轮转调度器持有的 TCB 表与「当前任务」元数据；队列逻辑见 `queues` 模块。
pub(super) struct TaskRegistry {
    // 首次 `__switch` 时的“伪当前”上下文占位，来自引导/单核 bring-up 路径。
    bootstrap_task_cx : TaskContext,
    task_table : TaskTable,
    current_task_id : Option<TaskId>,
}

impl TaskRegistry {
    pub(super) fn new() -> Self {
        Self { bootstrap_task_cx : TaskContext::zero_init(),
               task_table : TaskTable::new(),
               current_task_id : None }
    }

    pub(super) fn init(&mut self) {
        self.bootstrap_task_cx = TaskContext::zero_init();
        self.task_table
            .clear();
        self.current_task_id = None;
        self.task_table
            .insert(Box::new(TaskControlBlock::new_idle_task(IDLE_TASK_ID,
                                                             __wateros_idle_task_runtime_main)));
    }

    pub(super) fn spawn_kernel_task(&mut self, entry : KernelTaskEntry, arg : usize) -> TaskId {
        let task_id = self.task_table.allocate_id();
        let parent_id = self.current_task_id;
        self.task_table
            .insert(Box::new(TaskControlBlock::new_kernel_task(task_id, parent_id, entry, arg)));
        task_id
    }

    pub(super) fn spawn_user_task_spec(&mut self, spec : UserTask) -> TaskId {
        let task_id = self.task_table.allocate_id();
        log::trace!("[task-spawn] user spec id={} entry_pc={:#x} address_space_raw={:#x} \
                     image={:?} external_stack={:?}",
                    task_id,
                    spec.entry_pc(),
                    spec.address_space()
                        .map(|h| h.raw())
                        .unwrap_or(0),
                    spec.image(),
                    spec.stack());
        let parent_id = self.current_task_id;
        self.task_table
            .insert(Box::new(TaskControlBlock::new_user_task(task_id, parent_id, spec)));
        task_id
    }

    /// 从当前任务 fork 一个子用户任务。
    ///
    /// 子任务继承父任务的 trap 帧（a0 置 0）、使用独立地址空间
    /// (`new_aspace_ptr` / `new_satp`，由 `mm::kernel_mm::fork_user_aspace`
    /// 提供)、 独立内核栈。
    ///
    /// `child_stack` 非零时（clone），子任务初始用户 SP 设为该值。
    pub(super) fn fork_current(&mut self,
                               child_stack : usize,
                               new_aspace_ptr : usize,
                               new_satp : usize)
                               -> Option<TaskId> {
        let parent_id = self.current_task_id?;
        let child_id = self.task_table.allocate_id();

        let parent = self.task_table
                         .task(parent_id);
        log::trace!("[fork] parent={} child_stack={:#x} new_satp={:#x}",
                    parent_id,
                    child_stack,
                    new_satp);
        let child = parent.fork_from(child_id,
                                     child_stack,
                                     new_aspace_ptr,
                                     new_satp)?;

        self.task_table
            .insert(Box::new(child));
        log::trace!("[fork] child={} created parent={}",
                    child_id,
                    parent_id);
        Some(child_id)
    }

    /// 从当前用户任务 clone 一个同进程线程。
    pub(super) fn clone_current_thread(&mut self,
                                       child_stack : usize,
                                       tls : usize,
                                       set_tls : bool)
                                       -> Option<TaskId> {
        let parent_id = self.current_task_id?;
        let child_id = self.task_table.allocate_id();

        let parent = self.task_table
                         .task(parent_id);
        let child = parent.clone_thread_from(child_id, child_stack, tls, set_tls)?;

        self.task_table
            .insert(Box::new(child));
        log::trace!("[clone-thread] child={} created parent={} child_stack={:#x} set_tls={}",
                    child_id,
                    parent_id,
                    child_stack,
                    set_tls);
        Some(child_id)
    }

    /// execve：替换当前任务的地址空间、入口和栈。
    pub(super) fn execve_current(&mut self,
                                 entry_pc : usize,
                                 sp : usize,
                                 argc : usize,
                                 argv : usize,
                                 envp : usize,
                                 satp : usize,
                                 user_aspace_ptr : usize,
                                 image_info : task_api::UserImageInfo,
                                 stack_info : task_api::UserStack) {
        let current_id = self.current_task_id
                             .expect("execve requires a current task");
        self.task_table
            .task_mut(current_id)
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

    pub(super) fn first_switch_to(&mut self, next_task_id : TaskId) -> SwitchPair {
        let current_task_cx_ptr = &mut self.bootstrap_task_cx as *mut TaskContext;
        let next_task_cx_ptr = self.mark_running_and_set_current(next_task_id);
        (current_task_cx_ptr, next_task_cx_ptr)
    }

    pub(super) fn take_current_switch_out(&mut self) -> Option<(TaskId, *mut TaskContext)> {
        let current_task_id = self.current_task_id
                                  .take()?;
        let current_ptr = self.task_table
                              .task_mut(current_task_id)
                              .context_mut_ptr();
        Some((current_task_id, current_ptr))
    }

    pub(super) fn mark_running_and_set_current(&mut self, task_id : TaskId) -> *const TaskContext {
        self.task_table
            .task_mut(task_id)
            .mark_running();
        self.current_task_id = Some(task_id);
        self.task_table
            .task(task_id)
            .context_ptr()
    }

    pub(super) fn mark_ready(&mut self, task_id : TaskId) {
        if let Some(task) = self.task_table.task_mut_opt(task_id) {
            task.mark_ready();
        }
    }

    pub(super) fn mark_blocking(&mut self, task_id : TaskId, reason : TaskBlockReason) {
        if let Some(task) = self.task_table.task_mut_opt(task_id) {
            task.mark_blocking(reason);
        }
    }

    pub(super) fn mark_sleeping(&mut self, task_id : TaskId, wake_tick : TaskTick) {
        if let Some(task) = self.task_table.task_mut_opt(task_id) {
            task.mark_sleeping(wake_tick);
        }
    }

    pub(super) fn mark_exited(&mut self, task_id : TaskId, exit_code : TaskExitCode) {
        if let Some(task) = self.task_table.task_mut_opt(task_id) {
            task.mark_exited(exit_code);
        }
    }

    pub(super) fn ready_to_wake(&self, task_id : TaskId, current_tick : TaskTick) -> bool {
        self.task_table
            .task_opt(task_id)
            .is_some_and(|task| task.ready_to_wake(current_tick))
    }

    pub(super) fn is_idle(&self, task_id : TaskId) -> bool {
        self.task_table
            .task_opt(task_id)
            .is_some_and(TaskControlBlock::is_idle)
    }

    pub(super) fn state(&self, task_id : TaskId) -> Option<TaskState> {
        self.task_table
            .task_opt(task_id)
            .map(TaskControlBlock::state)
    }

    pub(super) fn wait_target_ready(&self, wait_handle : TaskWaitHandle) -> bool {
        match wait_handle.target() {
            TaskWaitTarget::WaitQueue(_) => false,
            // 目标任务已退出或槽位不存在（视为已结束）时，等待方无需阻塞。
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
        }
    }

    pub(super) fn parent_id(&self, task_id : TaskId) -> Option<TaskId> {
        self.task_table
            .task_opt(task_id)
            .and_then(TaskControlBlock::parent_id)
    }

    pub(super) fn is_schedulable(&self, task_id : TaskId) -> bool {
        if task_id == IDLE_TASK_ID {
            return self.task_table
                       .task_opt(task_id)
                       .is_some();
        }
        self.task_table
            .task_opt(task_id)
            .is_some_and(|task| !matches!(task.state(), TaskState::Exited(_)))
    }

    pub(super) fn has_child(&self, parent_id : TaskId) -> bool {
        self.task_table
            .iter_tasks()
            .any(|task| task.parent_id() == Some(parent_id))
    }

    pub(super) fn find_exited_child(&self, parent_id : TaskId) -> Option<TaskId> {
        self.task_table
            .iter_tasks()
            .find(|task| {
                task.parent_id() == Some(parent_id) && matches!(task.state(), TaskState::Exited(_))
            })
            .map(TaskControlBlock::id)
    }

    pub(super) fn account_tick_for_current(&mut self) {
        if let Some(current_task_id) = self.current_task_id {
            if !self.task_table
                    .task(current_task_id)
                    .is_idle()
            {
                self.task_table
                    .task_mut(current_task_id)
                    .account_tick();
            }
        }
    }

    pub(super) fn current_task_id(&self) -> Option<TaskId> { self.current_task_id }

    pub(super) fn current_task_snapshot(&self) -> Option<TaskSnapshot> {
        self.current_task_id
            .map(|task_id| {
                self.task_table
                    .task(task_id)
                    .snapshot()
            })
    }

    pub(super) fn task_snapshot(&self, task_id : TaskId) -> Option<TaskSnapshot> {
        self.task_table
            .task_opt(task_id)
            .map(TaskControlBlock::snapshot)
    }

    pub(super) fn current_task_kernel_stack_top(&self) -> Option<usize> {
        self.current_task_id
            .map(|task_id| {
                self.task_table
                    .task(task_id)
                    .kernel_stack_top()
            })
    }

    pub(super) fn current_task_address_space_raw(&self) -> usize {
        self.current_task_id
            .map(|task_id| {
                self.task_table
                    .task(task_id)
                    .user_address_space_raw()
            })
            .unwrap_or(0)
    }

    pub(super) fn current_task_user_aspace_ptr(&self) -> usize {
        self.current_task_id
            .map(|task_id| {
                self.task_table
                    .task(task_id)
                    .user_aspace_ptr()
            })
            .unwrap_or(0)
    }

    pub(super) fn current_task_user_address_space_token(&self) -> usize {
        self.current_task_address_space_raw()
    }

    pub(super) fn current_task_trap_return_address_space_token(&self) -> usize {
        self.current_task_id
            .map(|task_id| {
                self.task_table
                    .task(task_id)
                    .trap_return_address_space_token()
            })
            .unwrap_or(0)
    }

    pub(super) fn clear_wait_result(&mut self, task_id : TaskId) {
        if let Some(task) = self.task_table.task_mut_opt(task_id) {
            task.clear_wait_result();
        }
    }

    pub(super) fn finish_wait(&mut self, task_id : TaskId, result : TaskWaitResult) {
        if let Some(task) = self.task_table.task_mut_opt(task_id) {
            task.finish_wait(result);
        }
    }

    pub(super) fn take_current_wait_result(&mut self) -> TaskWaitResult {
        let current_task_id = self.current_task_id
                                  .expect("wait result can only be taken for a running task");
        self.task_table
            .task_mut(current_task_id)
            .take_wait_result()
    }

    pub(super) fn reap_task(&mut self, task_id : TaskId) -> Option<ExitedTask> {
        let task = self.task_table
                       .remove(task_id)?;
        task.exited_task()
    }

    pub(super) fn begin_current_trap_frame_access(&mut self,
                                                  trap_frame : TaskTrapFrame)
                                                  -> Option<*mut TaskTrapFrame> {
        let current_task_id = self.current_task_id?;
        if self.is_idle(current_task_id) {
            return None;
        }
        // 内核任务没有用户态 trap 帧需要保存，trap handler 直接使用栈上帧
        if !self.task_table
                .task(current_task_id)
                .is_user()
        {
            return None;
        }
        Some(self.task_table
                 .task_mut(current_task_id)
                 .begin_trap_frame_access(trap_frame))
    }

    pub(super) fn restore_current_trap_frame(&self, trap_frame : &mut TaskTrapFrame) -> bool {
        self.current_task_id
            .map(|current_task_id| {
                self.task_table
                    .task(current_task_id)
                    .restore_trap_frame_into(trap_frame)
            })
            .unwrap_or(false)
    }
}
