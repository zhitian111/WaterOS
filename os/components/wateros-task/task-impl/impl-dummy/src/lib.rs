#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use api_v0::{
    KernelTask, KernelTaskEntry, KernelTaskStart, TaskBlockReason, TaskExitCode, TaskId, TaskKind,
    TaskRuntimeStats, TaskSnapshot, TaskState, TaskTick, TaskTrapFrame,
};
use arch::task::ActiveArchTaskContext as TaskContext;

const KERNEL_TASK_STACK_SIZE: usize = 32 * 1024;

#[repr(align(16))]
struct AlignedKernelStack([u8; KERNEL_TASK_STACK_SIZE]);

pub struct TaskControlBlock {
    public: KernelTask,
    _start: Option<Box<KernelTaskStart>>,
    task_cx: TaskContext,
    _stack: Box<AlignedKernelStack>,
    is_idle: bool,
}

impl TaskControlBlock {
    pub fn new_kernel_task(
        id: TaskId,
        entry_stub: usize,
        entry: KernelTaskEntry,
        arg: usize,
    ) -> Self {
        Self::new(TaskKind::Kernel, id, entry_stub, entry, arg, false)
    }

    pub fn new_idle_task(id: TaskId, entry_stub: usize, entry: KernelTaskEntry) -> Self {
        let stack = Box::new(AlignedKernelStack([0; KERNEL_TASK_STACK_SIZE]));
        let stack_bottom = stack.0.as_ptr() as usize;
        let kernel_stack_top = align_down(stack_bottom + KERNEL_TASK_STACK_SIZE, 16);
        let task_cx = TaskContext::goto_entry(entry_stub, kernel_stack_top);
        Self {
            public: KernelTask {
                id,
                kind: TaskKind::Kernel,
                state: TaskState::Ready,
                trap_frame: None,
                kernel_stack_top,
                entry,
                stats: TaskRuntimeStats::default(),
            },
            _start: None,
            task_cx,
            _stack: stack,
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
        let stack = Box::new(AlignedKernelStack([0; KERNEL_TASK_STACK_SIZE]));
        let stack_bottom = stack.0.as_ptr() as usize;
        let kernel_stack_top = align_down(stack_bottom + KERNEL_TASK_STACK_SIZE, 16);
        let start = Box::new(KernelTaskStart::new(entry, arg));
        let task_start_ptr = start.as_ref() as *const KernelTaskStart as usize;
        let task_cx = TaskContext::goto_task_entry(entry_stub, kernel_stack_top, task_start_ptr);
        Self {
            public: KernelTask {
                id,
                kind,
                state: TaskState::Ready,
                trap_frame: None,
                kernel_stack_top,
                entry,
                stats: TaskRuntimeStats::default(),
            },
            _start: Some(start),
            task_cx,
            _stack: stack,
            is_idle,
        }
    }

    #[inline]
    pub fn id(&self) -> TaskId { self.public.id }

    #[inline]
    pub fn snapshot(&self) -> TaskSnapshot { self.public }

    #[inline]
    pub fn state(&self) -> TaskState { self.public.state }

    #[inline]
    pub fn is_idle(&self) -> bool { self.is_idle }

    #[inline]
    pub fn context_ptr(&self) -> *const TaskContext { &self.task_cx as *const TaskContext }

    #[inline]
    pub fn context_mut_ptr(&mut self) -> *mut TaskContext { &mut self.task_cx as *mut TaskContext }

    #[inline]
    pub fn mark_ready(&mut self) { self.public.state = TaskState::Ready; }

    #[inline]
    pub fn mark_running(&mut self) {
        self.public.state = TaskState::Running;
        self.public.stats.schedule_count = self.public.stats.schedule_count.saturating_add(1);
    }

    #[inline]
    pub fn mark_blocking(&mut self, reason: TaskBlockReason) {
        self.public.state = TaskState::Blocking(reason);
    }

    #[inline]
    pub fn mark_sleeping(&mut self, wake_tick: TaskTick) {
        self.public.state = TaskState::Sleeping { wake_tick };
    }

    #[inline]
    pub fn mark_exited(&mut self, exit_code: TaskExitCode) {
        self.public.state = TaskState::Exited(exit_code);
    }

    #[inline]
    pub fn account_tick(&mut self) {
        self.public.stats.tick_count = self.public.stats.tick_count.saturating_add(1);
    }

    #[inline]
    pub fn record_trap_frame(&mut self, trap_frame: TaskTrapFrame) {
        self.public.trap_frame = Some(trap_frame);
    }

    #[inline]
    pub fn restore_trap_frame_into(&self, trap_frame: &mut TaskTrapFrame) -> bool {
        if let Some(saved) = self.public.trap_frame {
            *trap_frame = saved;
            true
        } else {
            false
        }
    }

    #[inline]
    pub fn ready_to_wake(&self, current_tick: TaskTick) -> bool {
        matches!(
            self.public.state,
            TaskState::Sleeping { wake_tick } if wake_tick <= current_tick
        )
    }
}

#[inline]
const fn align_down(value: usize, align: usize) -> usize { value & !(align - 1) }
