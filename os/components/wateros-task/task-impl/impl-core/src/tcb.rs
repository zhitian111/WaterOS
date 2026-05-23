//! 任务控制块（**`TaskControlBlock`**）与任务类型专属资源：把 `task_api`
//! 中的规格落到具体栈、trap 帧与地址空间句柄上。
//!
//! 调度器只通过 `task_api` 抽象操作本模块类型。

use alloc::boxed::Box;
use api_v0::{
    ExitedTask, KernelStack, KernelTaskEntry, TaskBlockReason, TaskBootstrap, TaskExitCode, TaskId,
    TaskKind, TaskRuntimeStats, TaskSnapshot, TaskState, TaskTick, TaskTrapSnapshot,
    TaskWaitResult, UserTask,
};
use arch::task::{ActiveArchTaskContext as TaskContext, ArchTaskContext};
use arch::trap::{ActiveTrapFrame as TaskTrapFrame, TrapContextRead, TrapContextWrite};

unsafe extern "C" {
    fn __arch_task_entry();
    fn __arch_user_task_entry();
}

// ── 任务类型专属资源 ──────────────────────────────────────────────

enum TaskInner {
    Idle,
    Kernel(KernelResources),
    User(UserResources),
}

struct KernelResources {
    kernel_stack : KernelStack,
    bootstrap : Box<TaskBootstrap>,
}

struct UserResources {
    kernel_stack : KernelStack,
    trap_frame : TaskTrapFrame,
    user : UserTask,
}

impl UserResources {
    fn new(kernel_stack : KernelStack, user : UserTask) -> Self {
        let token = user.address_space()
                        .expect("user task requires an address space (use with_address_space)")
                        .raw();
        let entry_pc = user.entry_pc();
        let stack = user.stack()
                        .expect("UserTask must have a stack (use with_stack)");
        let mut trap_frame = TaskTrapFrame::default();
        trap_frame.prepare_user_return(entry_pc,
                                       initial_user_sp(stack.top(), stack.bottom()));
        trap_frame.set_return_address_space_token(token);
        Self { kernel_stack,
               trap_frame,
               user }
    }
}

// ── 辅助函数 ─────────────────────────────────────────────────────

fn initial_user_sp(top_exclusive : usize, bottom : usize) -> usize {
    let sp = top_exclusive.saturating_sub(16);
    if sp < bottom {
        bottom
    } else {
        sp
    }
}

fn trap_snapshot(trap_frame : TaskTrapFrame, user_aspace_ptr : usize) -> TaskTrapSnapshot {
    TaskTrapSnapshot::new(<TaskTrapFrame as TrapContextRead>::raw_cause(&trap_frame),
                          <TaskTrapFrame as TrapContextRead>::user_pc(&trap_frame),
                          <TaskTrapFrame as TrapContextRead>::user_sp(&trap_frame),
                          user_aspace_ptr,
                          <TaskTrapFrame as TrapContextRead>::fault_addr(&trap_frame),
                          <TaskTrapFrame as TrapContextRead>::returns_to_user(&trap_frame))
}

// ── 任务控制块 ───────────────────────────────────────────────────

/// 调度器持有的任务控制块。
pub struct TaskControlBlock {
    id : TaskId,
    parent_id : Option<TaskId>,
    state : TaskState,
    stats : TaskRuntimeStats,
    wait_result : Option<TaskWaitResult>,
    task_cx : TaskContext,
    inner : TaskInner,
}

impl TaskControlBlock {
    /// 创建一个普通内核任务，并初始化其启动上下文。
    pub fn new_kernel_task(id : TaskId,
                           parent_id : Option<TaskId>,
                           entry : KernelTaskEntry,
                           arg : usize)
                           -> Self {
        let kernel_stack = KernelStack::new();
        let bootstrap = Box::new(TaskBootstrap::new(entry, arg));
        let bootstrap_ptr = bootstrap.as_ref() as *const TaskBootstrap as usize;
        let task_cx = TaskContext::goto_task_entry(__arch_task_entry as *const () as usize,
                                                   kernel_stack.top(),
                                                   bootstrap_ptr);
        Self { id,
               parent_id,
               state : TaskState::Ready,
               stats : TaskRuntimeStats::default(),
               wait_result : None,
               task_cx,
               inner : TaskInner::Kernel(KernelResources { kernel_stack,
                                                           bootstrap }) }
    }

    /// 创建 idle 任务。
    pub fn new_idle_task(id : TaskId, entry : KernelTaskEntry) -> Self {
        let kernel_stack = KernelStack::new();
        let bootstrap = Box::new(TaskBootstrap::new(entry, 0));
        let bootstrap_ptr = bootstrap.as_ref() as *const TaskBootstrap as usize;
        let task_cx = TaskContext::goto_task_entry(__arch_task_entry as *const () as usize,
                                                   kernel_stack.top(),
                                                   bootstrap_ptr);
        Self { id,
               parent_id : None,
               state : TaskState::Ready,
               stats : TaskRuntimeStats::default(),
               wait_result : None,
               task_cx,
               inner : TaskInner::Idle }
    }

    /// 创建一个用户任务。
    pub fn new_user_task(id : TaskId, parent_id : Option<TaskId>, user : UserTask) -> Self {
        let kernel_stack = KernelStack::new();
        let task_cx = TaskContext::goto_entry(__arch_user_task_entry as *const () as usize,
                                              kernel_stack.top());
        let user = UserResources::new(kernel_stack, user);
        Self { id,
               parent_id,
               state : TaskState::Ready,
               stats : TaskRuntimeStats::default(),
               wait_result : None,
               task_cx,
               inner : TaskInner::User(user) }
    }

    // ── 通用访问器 ──────────────────────────────────────────────

    #[inline]
    pub fn id(&self) -> TaskId { self.id }

    #[inline]
    pub fn parent_id(&self) -> Option<TaskId> { self.parent_id }

    #[inline]
    pub fn state(&self) -> TaskState { self.state }

    #[inline]
    pub fn is_idle(&self) -> bool { matches!(self.inner, TaskInner::Idle) }

    #[inline]
    pub fn context_ptr(&self) -> *const TaskContext { &self.task_cx as *const TaskContext }

    #[inline]
    pub fn context_mut_ptr(&mut self) -> *mut TaskContext { &mut self.task_cx as *mut TaskContext }

    #[inline]
    pub fn snapshot(&self) -> TaskSnapshot {
        let kind = match &self.inner {
            TaskInner::Idle | TaskInner::Kernel(_) => TaskKind::Kernel,
            TaskInner::User(_) => TaskKind::User,
        };
        let trap_frame = match &self.inner {
            TaskInner::User(u) => Some(trap_snapshot(u.trap_frame,
                                                     u.user
                                                      .user_aspace_ptr()
                                                      .unwrap_or(0))),
            _ => None,
        };
        TaskSnapshot { id : self.id,
                       parent_id : self.parent_id,
                       kind,
                       state : self.state,
                       trap_frame,
                       stats : self.stats }
    }

    // ── 内核栈 ──────────────────────────────────────────────────

    #[inline]
    pub fn kernel_stack_top(&self) -> usize {
        match &self.inner {
            TaskInner::Idle => 0,
            TaskInner::Kernel(k) => k.kernel_stack.top(),
            TaskInner::User(u) => u.kernel_stack.top(),
        }
    }

    // ── 用户任务方法 ─────────────────────────────────────────────

    #[inline]
    pub fn user_stack_top(&self) -> Option<usize> {
        match &self.inner {
            TaskInner::User(u) => u.user
                                   .stack()
                                   .map(|s| s.top()),
            _ => None,
        }
    }

    #[inline]
    pub fn user_address_space_raw(&self) -> usize {
        match &self.inner {
            TaskInner::User(u) => u.user
                                   .address_space()
                                   .map(|h| h.raw())
                                   .unwrap_or(0),
            _ => 0,
        }
    }

    #[inline]
    pub fn user_aspace_ptr(&self) -> usize {
        match &self.inner {
            TaskInner::User(u) => u.user
                                   .user_aspace_ptr()
                                   .unwrap_or(0),
            _ => 0,
        }
    }

    // ── Bootstrap（内核任务首次启动用） ──────────────────────────

    #[inline]
    pub fn bootstrap_ptr(&self) -> Option<usize> {
        match &self.inner {
            TaskInner::Kernel(k) => Some(k.bootstrap.as_ref() as *const TaskBootstrap as usize),
            _ => None,
        }
    }

    // ── Trap frame 访问 ─────────────────────────────────────────

    #[inline]
    pub fn begin_trap_frame_access(&mut self, trap_frame : TaskTrapFrame) -> *mut TaskTrapFrame {
        match &mut self.inner {
            TaskInner::User(u) => {
                u.trap_frame = trap_frame;
                &mut u.trap_frame as *mut TaskTrapFrame
            }
            _ => panic!("begin_trap_frame_access called on non-user task"),
        }
    }

    #[inline]
    pub fn restore_trap_frame_into(&self, trap_frame : &mut TaskTrapFrame) -> bool {
        match &self.inner {
            TaskInner::User(u) => {
                *trap_frame = u.trap_frame;
                true
            }
            _ => false,
        }
    }

    // ── 退出 ────────────────────────────────────────────────────

    #[inline]
    pub fn exited_task(&self) -> Option<ExitedTask> {
        let TaskState::Exited(exit_code) = self.state else {
            return None;
        };
        let kind = match &self.inner {
            TaskInner::Idle | TaskInner::Kernel(_) => TaskKind::Kernel,
            TaskInner::User(_) => TaskKind::User,
        };
        let trap_frame = match &self.inner {
            TaskInner::User(u) => Some(trap_snapshot(u.trap_frame,
                                                     u.user
                                                      .user_aspace_ptr()
                                                      .unwrap_or(0))),
            _ => None,
        };
        Some(ExitedTask { id : self.id,
                          parent_id : self.parent_id,
                          kind,
                          exit_code,
                          trap_frame,
                          stats : self.stats })
    }

    // ── 状态变迁 ────────────────────────────────────────────────

    #[inline]
    pub fn mark_ready(&mut self) { self.state = TaskState::Ready; }

    #[inline]
    pub fn mark_running(&mut self) {
        self.state = TaskState::Running;
        self.stats
            .schedule_count = self.stats
                                  .schedule_count
                                  .saturating_add(1);
    }

    #[inline]
    pub fn mark_blocking(&mut self, reason : TaskBlockReason) {
        self.state = TaskState::Blocking(reason);
    }

    #[inline]
    pub fn mark_sleeping(&mut self, wake_tick : TaskTick) {
        self.state = TaskState::Sleeping { wake_tick };
    }

    #[inline]
    pub fn mark_exited(&mut self, exit_code : TaskExitCode) {
        self.state = TaskState::Exited(exit_code);
    }

    #[inline]
    pub fn account_tick(&mut self) {
        self.stats
            .tick_count = self.stats
                              .tick_count
                              .saturating_add(1);
    }

    // ── 等待 ────────────────────────────────────────────────────

    #[inline]
    pub fn clear_wait_result(&mut self) { self.wait_result = None; }

    #[inline]
    pub fn finish_wait(&mut self, result : TaskWaitResult) { self.wait_result = Some(result); }

    #[inline]
    pub fn take_wait_result(&mut self) -> TaskWaitResult {
        self.wait_result
            .take()
            .unwrap_or(TaskWaitResult::Woken)
    }

    #[inline]
    pub fn ready_to_wake(&self, current_tick : TaskTick) -> bool {
        matches!(self.state, TaskState::Sleeping { wake_tick } if wake_tick <= current_tick)
    }
}

// ----- end of TaskControlBlock impl -----
