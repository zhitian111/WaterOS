//! 任务控制块（**`TaskControlBlock`**）与用户态资源快照：把 `task_api` 中的规格落到具体栈、trap 帧与地址空间句柄上。
//!
//! 调度器只通过 `task_api` 抽象操作本模块类型；**就绪顺序与时间片**不在此文件实现。

use alloc::boxed::Box;
use api_v0::{
    AddressSpaceHandle, ExitedTask, KernelTaskEntry, TaskBlockReason, TaskExitCode, TaskId,
    TaskKind, TaskRuntimeStats, TaskSnapshot, TaskState, TaskTick, TaskTrapSnapshot,
    TaskWaitResult, UserImageInfo, UserTaskEntryPc, UserTaskResources as UserTaskResourcesSnapshot,
    UserTaskSpec,
};
use arch::task::{ActiveArchTaskContext as TaskContext, ArchTaskContext};
use arch::trap::{ActiveTrapFrame as TaskTrapFrame, TrapContextRead, TrapContextWrite};

use crate::stack::{KernelStack, UserStack};
use crate::TaskBootstrap;

// 用户栈既可由本 crate 分配，也可由创建方通过 `UserTaskSpec::with_external_stack` 提供已映射区间。
enum UserStackBacking {
    Kernel(UserStack),
    External {
        bottom : usize,
        top : usize,
    },
}

// 与 `task_api::UserTaskResources` 快照对应的内部可变表示；仅在 TCB 内使用。
struct UserTaskResources {
    entry_pc : UserTaskEntryPc,
    user_stack : UserStackBacking,
    address_space : Option<AddressSpaceHandle>,
    image : Option<UserImageInfo>,
    user_aspace_ptr : usize,
}
unsafe extern "C" {
    /// 内核任务通用入口桩：从栈上取出 `TaskBootstrap` 并转入 `TaskBootstrap::run`。
    fn __arch_task_entry();
    /// 用户任务入口桩：装配完成后由调度器经 `__wateros_task_runtime_enter_current_user_task` 进入用户态。
    fn __arch_user_task_entry();
}

/// 用户栈映射为 `[bottom, top)`（`top` 为不包含上界）时，初始 `sp` 必须落在已映射页内并满足 RISC-V psABI 的 16 字节对齐。
#[inline]
fn initial_user_sp(top_exclusive : usize, bottom : usize) -> usize {
    let sp = top_exclusive.saturating_sub(16);
    if sp < bottom {
        bottom
    } else {
        sp
    }
}

impl UserTaskResources {
    fn new(spec : UserTaskSpec) -> Self {
        let user_stack = if let Some((bottom, top)) = spec.external_stack() {
            UserStackBacking::External { bottom, top }
        } else {
            UserStackBacking::Kernel(UserStack::new())
        };
        Self { entry_pc : spec.entry_pc(),
               user_stack,
               address_space : spec.address_space(),
               image : spec.image(),
               user_aspace_ptr : spec.user_aspace_ptr().unwrap_or(0) }
    }

    fn entry_pc(&self) -> UserTaskEntryPc { self.entry_pc }

    fn user_stack_top(&self) -> usize {
        match &self.user_stack {
            UserStackBacking::Kernel(s) => s.top(),
            UserStackBacking::External { top, .. } => *top,
        }
    }

    fn user_stack_bottom(&self) -> usize {
        match &self.user_stack {
            UserStackBacking::Kernel(s) => s.bottom(),
            UserStackBacking::External { bottom, .. } => *bottom,
        }
    }

    fn user_stack_size(&self) -> usize {
        match &self.user_stack {
            UserStackBacking::Kernel(s) => s.size(),
            UserStackBacking::External { bottom, top } => top.saturating_sub(*bottom),
        }
    }

    fn snapshot(&self) -> UserTaskResourcesSnapshot {
        UserTaskResourcesSnapshot { entry_pc : self.entry_pc,
                                    user_stack_bottom : self.user_stack_bottom(),
                                    user_stack_top : self.user_stack_top(),
                                    user_stack_size : self.user_stack_size(),
                                    address_space : self.address_space,
                                    image : self.image,
                                    user_aspace_ptr : self.user_aspace_ptr }
    }

    fn address_space_raw(&self) -> usize {
        self.address_space
            .map(|h| h.raw())
            .unwrap_or(0)
    }
}

// 从架构 trap 帧读出语义字段，供 `TaskSnapshot` / `ExitedTask` 使用。
fn trap_snapshot(trap_frame : TaskTrapFrame) -> TaskTrapSnapshot {
    TaskTrapSnapshot::new(<TaskTrapFrame as TrapContextRead>::raw_cause(&trap_frame),
                          <TaskTrapFrame as TrapContextRead>::user_pc(&trap_frame),
                          <TaskTrapFrame as TrapContextRead>::user_sp(&trap_frame),
                          <TaskTrapFrame as TrapContextRead>::fault_addr(&trap_frame),
                          <TaskTrapFrame as TrapContextRead>::returns_to_user(&trap_frame))
}

/// 调度器持有的任务控制块。
pub struct TaskControlBlock {
    id : TaskId,
    kind : TaskKind,
    state : TaskState,
    stats : TaskRuntimeStats,
    trap_frame : Option<TaskTrapFrame>,
    wait_result : Option<TaskWaitResult>,
    task_cx : TaskContext,
    kernel_stack : KernelStack,
    user_resources : Option<UserTaskResources>,
    bootstrap : Option<Box<TaskBootstrap>>,
    is_idle : bool,
}

impl TaskControlBlock {
    /// 创建一个普通内核任务，并初始化其启动上下文。
    pub fn new_kernel_task(id : TaskId, entry : KernelTaskEntry, arg : usize) -> Self {
        let kernel_stack = KernelStack::new();
        let bootstrap = Box::new(TaskBootstrap::new(entry, arg));
        let bootstrap_ptr = bootstrap.as_ref() as *const TaskBootstrap as usize;
        let task_cx = TaskContext::goto_task_entry(__arch_task_entry as *const () as usize,
                                                   kernel_stack.top(),
                                                   bootstrap_ptr);
        Self::new(id,
                  TaskKind::Kernel,
                  task_cx,
                  None,
                  kernel_stack,
                  None,
                  Some(bootstrap),
                  false)
    }

    /// 创建 idle 任务。
    pub fn new_idle_task(id : TaskId, entry : KernelTaskEntry) -> Self {
        let kernel_stack = KernelStack::new();
        let bootstrap = Box::new(TaskBootstrap::new(entry, 0));
        let bootstrap_ptr = bootstrap.as_ref() as *const TaskBootstrap as usize;
        let task_cx = TaskContext::goto_task_entry(__arch_task_entry as *const () as usize,
                                                   kernel_stack.top(),
                                                   bootstrap_ptr);
        Self::new(id,
                  TaskKind::Kernel,
                  task_cx,
                  None,
                  kernel_stack,
                  None,
                  Some(bootstrap),
                  true)
    }

    /// 创建一个最小用户任务骨架。
    pub fn new_user_task(id : TaskId, spec : UserTaskSpec) -> Self {
        let kernel_stack = KernelStack::new();
        let user_resources = UserTaskResources::new(spec);
        let task_cx = TaskContext::goto_entry(__arch_user_task_entry as *const () as usize,
                                              kernel_stack.top());
        let mut trap_frame = TaskTrapFrame::default();
        trap_frame.prepare_user_return(
            user_resources.entry_pc(),
            initial_user_sp(user_resources.user_stack_top(),
                            user_resources.user_stack_bottom()),
        );
        Self::new(id,
                  TaskKind::User,
                  task_cx,
                  Some(trap_frame),
                  kernel_stack,
                  Some(user_resources),
                  None,
                  false)
    }

    fn new(id : TaskId,
           kind : TaskKind,
           task_cx : TaskContext,
           trap_frame : Option<TaskTrapFrame>,
           kernel_stack : KernelStack,
           user_resources : Option<UserTaskResources>,
           bootstrap : Option<Box<TaskBootstrap>>,
           is_idle : bool)
           -> Self {
        Self { id,
               kind,
               state : TaskState::Ready,
               stats : TaskRuntimeStats::default(),
               trap_frame,
               wait_result : None,
               task_cx,
               kernel_stack,
               user_resources,
               bootstrap,
               is_idle }
    }

    #[inline]
    /// 返回任务号。
    pub fn id(&self) -> TaskId { self.id }

    #[inline]
    /// 生成对外可见的稳定任务快照。
    pub fn snapshot(&self) -> TaskSnapshot {
        TaskSnapshot { id : self.id,
                       kind : self.kind,
                       state : self.state,
                       trap_frame : self.trap_frame
                                        .map(trap_snapshot),
                       stats : self.stats,
                       user_resources : self.user_resources_snapshot() }
    }

    #[inline]
    /// 返回当前任务状态。
    pub fn state(&self) -> TaskState { self.state }

    #[inline]
    /// 判断该任务是否为 idle 任务。
    pub fn is_idle(&self) -> bool { self.is_idle }

    #[inline]
    /// 返回只读任务上下文指针，供汇编切换路径使用。
    pub fn context_ptr(&self) -> *const TaskContext { &self.task_cx as *const TaskContext }

    #[inline]
    /// 返回可写任务上下文指针，供汇编切换路径使用。
    pub fn context_mut_ptr(&mut self) -> *mut TaskContext { &mut self.task_cx as *mut TaskContext }

    #[inline]
    /// 返回任务内核栈顶地址。
    pub fn kernel_stack_top(&self) -> usize {
        self.kernel_stack
            .top()
    }

    #[inline]
    /// 若该任务持有用户栈，则返回其栈顶地址。
    pub fn user_stack_top(&self) -> Option<usize> {
        self.user_resources
            .as_ref()
            .map(UserTaskResources::user_stack_top)
    }

    /// 用户任务 `AddressSpaceHandle::raw()`；非用户任务或未设置时返回 `0`。
    #[inline]
    pub fn user_address_space_raw(&self) -> usize {
        if self.kind != TaskKind::User {
            return 0;
        }
        self.user_resources
            .as_ref()
            .map(UserTaskResources::address_space_raw)
            .unwrap_or(0)
    }

    #[inline]
    /// 若为用户任务，则返回其资源快照。
    pub fn user_resources_snapshot(&self) -> Option<UserTaskResourcesSnapshot> {
        self.user_resources
            .as_ref()
            .map(UserTaskResources::snapshot)
    }

    #[inline]
    /// 返回 bootstrap 对象指针，供任务首次启动时传给入口桩。
    pub fn bootstrap_ptr(&self) -> Option<usize> {
        self.bootstrap
            .as_ref()
            .map(|bootstrap| bootstrap.as_ref() as *const TaskBootstrap as usize)
    }

    #[inline]
    /// 如果任务已经退出，导出一份可回收的退出信息。
    pub fn exited_task(&self) -> Option<ExitedTask> {
        let TaskState::Exited(exit_code) = self.state else {
            return None;
        };
        Some(ExitedTask { id : self.id,
                          kind : self.kind,
                          exit_code,
                          trap_frame : self.trap_frame
                                           .map(trap_snapshot),
                          stats : self.stats,
                          user_resources : self.user_resources_snapshot() })
    }

    #[inline]
    /// 将任务状态置为 Ready。
    pub fn mark_ready(&mut self) { self.state = TaskState::Ready; }

    #[inline]
    /// 将任务状态置为 Running，并累计一次调度计数。
    pub fn mark_running(&mut self) {
        self.state = TaskState::Running;
        self.stats
            .schedule_count = self.stats
                                  .schedule_count
                                  .saturating_add(1);
    }

    #[inline]
    /// 将任务状态置为阻塞，并记录阻塞原因。
    pub fn mark_blocking(&mut self, reason : TaskBlockReason) {
        self.state = TaskState::Blocking(reason);
    }

    #[inline]
    /// 将任务状态置为睡眠，直到指定 tick。
    pub fn mark_sleeping(&mut self, wake_tick : TaskTick) {
        self.state = TaskState::Sleeping { wake_tick };
    }

    #[inline]
    /// 将任务状态置为已退出。
    pub fn mark_exited(&mut self, exit_code : TaskExitCode) {
        self.state = TaskState::Exited(exit_code);
    }

    #[inline]
    /// 为任务累计一个运行 tick。
    pub fn account_tick(&mut self) {
        self.stats
            .tick_count = self.stats
                              .tick_count
                              .saturating_add(1);
    }

    #[inline]
    /// 保存最近一次 trap 现场到任务对象中。
    pub fn record_trap_frame(&mut self, trap_frame : TaskTrapFrame) {
        self.trap_frame = Some(trap_frame);
    }

    #[inline]
    /// 将给定 trap 现场装载为任务当前的权威 trap frame，并返回其可写指针。
    pub fn begin_trap_frame_access(&mut self, trap_frame : TaskTrapFrame) -> *mut TaskTrapFrame {
        self.trap_frame = Some(trap_frame);
        self.trap_frame
            .as_mut()
            .map(|trap_frame| trap_frame as *mut TaskTrapFrame)
            .expect("trap frame must exist after begin_trap_frame_access")
    }

    #[inline]
    /// 清除任务上次等待返回结果。
    pub fn clear_wait_result(&mut self) { self.wait_result = None; }

    #[inline]
    /// 记录一次等待结束结果。
    pub fn finish_wait(&mut self, result : TaskWaitResult) { self.wait_result = Some(result); }

    #[inline]
    /// 取出等待结果；若未显式记录则按正常唤醒处理。
    pub fn take_wait_result(&mut self) -> TaskWaitResult {
        self.wait_result
            .take()
            .unwrap_or(TaskWaitResult::Woken)
    }

    #[inline]
    /// 将任务保存的 trap 现场恢复到给定 trap frame 缓冲区。
    pub fn restore_trap_frame_into(&self, trap_frame : &mut TaskTrapFrame) -> bool {
        if let Some(saved) = self.trap_frame {
            *trap_frame = saved;
            true
        } else {
            false
        }
    }

    #[inline]
    /// 判断睡眠中的任务是否已经到达可唤醒时间。
    pub fn ready_to_wake(&self, current_tick : TaskTick) -> bool {
        matches!(
            self.state,
            TaskState::Sleeping { wake_tick } if wake_tick <= current_tick
        )
    }
}
