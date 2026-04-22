#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use api_v0::{
    ExitedTask, KernelTaskEntry, TaskBlockReason, TaskExitCode, TaskId, TaskKind,
    TaskRuntimeStats, TaskSnapshot, TaskState, TaskTick, TaskTrapFrame, TaskWaitResult,
};
use arch::task::ActiveArchTaskContext as TaskContext;

mod runtime;

pub use runtime::TaskBootstrap;

const KERNEL_TASK_STACK_SIZE: usize = 32 * 1024;

#[repr(align(16))]
struct AlignedKernelStack([u8; KERNEL_TASK_STACK_SIZE]);

/// 内核任务独占的内核栈封装。
pub struct KernelStack {
    storage: Box<AlignedKernelStack>,
    top: usize,
}

impl KernelStack {
    fn new() -> Self {
        let storage = Box::new(AlignedKernelStack([0; KERNEL_TASK_STACK_SIZE]));
        let stack_bottom = storage.0.as_ptr() as usize;
        let top = align_down(stack_bottom + KERNEL_TASK_STACK_SIZE, 16);
        Self { storage, top }
    }

    #[inline]
    /// 返回当前内核栈的栈顶地址。
    pub fn top(&self) -> usize {
        debug_assert_eq!(
            align_down(self.storage.0.as_ptr() as usize + KERNEL_TASK_STACK_SIZE, 16),
            self.top
        );
        self.top
    }
}

/// 调度器持有的任务控制块。
pub struct TaskControlBlock {
    id: TaskId,
    kind: TaskKind,
    state: TaskState,
    stats: TaskRuntimeStats,
    trap_frame: Option<TaskTrapFrame>,
    wait_result: Option<TaskWaitResult>,
    task_cx: TaskContext,
    kernel_stack: KernelStack,
    bootstrap: Option<Box<TaskBootstrap>>,
    is_idle: bool,
}

impl TaskControlBlock {
    /// 创建一个普通内核任务，并初始化其启动上下文。
    pub fn new_kernel_task(
        id: TaskId,
        entry_stub: usize,
        entry: KernelTaskEntry,
        arg: usize,
    ) -> Self {
        Self::new(TaskKind::Kernel, id, entry_stub, entry, arg, false)
    }

    /// 创建 idle 任务。
    pub fn new_idle_task(id: TaskId, entry_stub: usize, entry: KernelTaskEntry) -> Self {
        let kernel_stack = KernelStack::new();
        let task_cx = TaskContext::goto_entry(entry_stub, kernel_stack.top());
        let _ = entry;
        Self {
            id,
            kind: TaskKind::Kernel,
            state: TaskState::Ready,
            stats: TaskRuntimeStats::default(),
            trap_frame: None,
            wait_result: None,
            task_cx,
            kernel_stack,
            bootstrap: None,
            is_idle: true,
        }
    }

    fn new(
        kind: TaskKind,
        id: TaskId,
        entry_stub: usize,
        entry: KernelTaskEntry,
        arg: usize,
        is_idle: bool,
    ) -> Self {
        let kernel_stack = KernelStack::new();
        let bootstrap = Box::new(TaskBootstrap::new(entry, arg));
        let bootstrap_ptr = bootstrap.as_ref() as *const TaskBootstrap as usize;
        let task_cx = TaskContext::goto_task_entry(entry_stub, kernel_stack.top(), bootstrap_ptr);
        Self {
            id,
            kind,
            state: TaskState::Ready,
            stats: TaskRuntimeStats::default(),
            trap_frame: None,
            wait_result: None,
            task_cx,
            kernel_stack,
            bootstrap: Some(bootstrap),
            is_idle,
        }
    }

    #[inline]
    /// 返回任务号。
    pub fn id(&self) -> TaskId { self.id }

    #[inline]
    /// 生成对外可见的稳定任务快照。
    pub fn snapshot(&self) -> TaskSnapshot {
        TaskSnapshot {
            id: self.id,
            kind: self.kind,
            state: self.state,
            trap_frame: self.trap_frame,
            stats: self.stats,
        }
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
    pub fn kernel_stack_top(&self) -> usize { self.kernel_stack.top() }

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
        Some(ExitedTask {
            id: self.id,
            kind: self.kind,
            exit_code,
            trap_frame: self.trap_frame,
            stats: self.stats,
        })
    }

    #[inline]
    /// 将任务状态置为 Ready。
    pub fn mark_ready(&mut self) { self.state = TaskState::Ready; }

    #[inline]
    /// 将任务状态置为 Running，并累计一次调度计数。
    pub fn mark_running(&mut self) {
        self.state = TaskState::Running;
        self.stats.schedule_count = self.stats.schedule_count.saturating_add(1);
    }

    #[inline]
    /// 将任务状态置为阻塞，并记录阻塞原因。
    pub fn mark_blocking(&mut self, reason: TaskBlockReason) { self.state = TaskState::Blocking(reason); }

    #[inline]
    /// 将任务状态置为睡眠，直到指定 tick。
    pub fn mark_sleeping(&mut self, wake_tick: TaskTick) { self.state = TaskState::Sleeping { wake_tick }; }

    #[inline]
    /// 将任务状态置为已退出。
    pub fn mark_exited(&mut self, exit_code: TaskExitCode) { self.state = TaskState::Exited(exit_code); }

    #[inline]
    /// 为任务累计一个运行 tick。
    pub fn account_tick(&mut self) { self.stats.tick_count = self.stats.tick_count.saturating_add(1); }

    #[inline]
    /// 保存最近一次 trap 现场到任务对象中。
    pub fn record_trap_frame(&mut self, trap_frame: TaskTrapFrame) { self.trap_frame = Some(trap_frame); }

    #[inline]
    /// 清除任务上次等待返回结果。
    pub fn clear_wait_result(&mut self) { self.wait_result = None; }

    #[inline]
    /// 记录一次等待结束结果。
    pub fn finish_wait(&mut self, result: TaskWaitResult) { self.wait_result = Some(result); }

    #[inline]
    /// 取出等待结果；若未显式记录则按正常唤醒处理。
    pub fn take_wait_result(&mut self) -> TaskWaitResult {
        self.wait_result.take().unwrap_or(TaskWaitResult::Woken)
    }

    #[inline]
    /// 将任务保存的 trap 现场恢复到给定 trap frame 缓冲区。
    pub fn restore_trap_frame_into(&self, trap_frame: &mut TaskTrapFrame) -> bool {
        if let Some(saved) = self.trap_frame {
            *trap_frame = saved;
            true
        } else {
            false
        }
    }

    #[inline]
    /// 判断睡眠中的任务是否已经到达可唤醒时间。
    pub fn ready_to_wake(&self, current_tick: TaskTick) -> bool {
        matches!(
            self.state,
            TaskState::Sleeping { wake_tick } if wake_tick <= current_tick
        )
    }
}

#[inline]
const fn align_down(value: usize, align: usize) -> usize { value & !(align - 1) }
