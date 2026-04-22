#![no_std]

pub type TaskId = usize;
pub type TaskTick = u64;
pub type TaskExitCode = isize;
pub type WaitQueueId = usize;
pub type KernelTaskEntry = extern "C" fn(usize) -> !;

pub const IDLE_TASK_ID: TaskId = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskKind {
    Kernel,
    User,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskBlockReason {
    Yield,
    Sleep,
    WaitQueue(WaitQueueId),
    UserSyscall,
    Manual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskState {
    Ready,
    Running,
    Blocking(TaskBlockReason),
    Sleeping { wake_tick: TaskTick },
    Exited(TaskExitCode),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleReason {
    StartFirst,
    Yield,
    Tick,
    Block(TaskBlockReason),
    Sleep(TaskTick),
    Exit(TaskExitCode),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskRuntimeStats {
    pub schedule_count: usize,
    pub tick_count: usize,
}

/// A task-owned snapshot of the most recent trap context.
///
/// The layout intentionally mirrors the current RISC-V `TrapContext` so the
/// trap path can copy the saved frame into the task object without field-by-
/// field translation.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskTrapFrame {
    pub x: [usize; 32],
    pub sstatus: usize,
    pub sepc: usize,
    pub scause: usize,
    pub stval: usize,
}

impl TaskTrapFrame {
    #[inline]
    pub const fn raw_cause(&self) -> usize { self.scause }

    #[inline]
    pub const fn user_pc(&self) -> usize { self.sepc }

    #[inline]
    pub const fn fault_addr(&self) -> usize { self.stval }
}

/// A task-start descriptor used by the task runtime to bootstrap a task from
/// an opaque arch-specific context into the final task entry call.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KernelTaskStart {
    pub entry: KernelTaskEntry,
    pub arg: usize,
}

impl KernelTaskStart {
    #[inline]
    pub const fn new(entry: KernelTaskEntry, arg: usize) -> Self { Self { entry, arg } }
}

#[derive(Clone, Copy, Debug)]
pub struct KernelTask {
    pub id: TaskId,
    pub kind: TaskKind,
    pub state: TaskState,
    pub trap_frame: Option<TaskTrapFrame>,
    pub kernel_stack_top: usize,
    pub entry: KernelTaskEntry,
    pub stats: TaskRuntimeStats,
}

pub type TaskSnapshot = KernelTask;
