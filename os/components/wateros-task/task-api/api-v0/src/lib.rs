#![no_std]

pub use arch::task::ActiveArchTaskContext as TaskContext;

pub type TaskId = usize;
pub type TaskTick = u64;
pub type TaskExitCode = isize;
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
    WaitQueue,
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

#[derive(Clone, Copy, Debug)]
pub struct KernelTask {
    pub id: TaskId,
    pub kind: TaskKind,
    pub state: TaskState,
    pub task_cx: TaskContext,
    pub kernel_stack_top: usize,
    pub entry: KernelTaskEntry,
    pub stats: TaskRuntimeStats,
}

pub type TaskSnapshot = KernelTask;
