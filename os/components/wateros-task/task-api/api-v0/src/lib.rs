#![no_std]

pub type TaskId = usize;
pub type KernelTaskEntry = extern "C" fn(usize) -> !;

pub const IDLE_TASK_ID: TaskId = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskStatus {
    Ready,
    Running,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TaskContext {
    pub ra: usize,
    pub sp: usize,
    pub s: [usize; 12],
}

impl TaskContext {
    #[inline]
    pub const fn zero_init() -> Self {
        Self {
            ra: 0,
            sp: 0,
            s: [0; 12],
        }
    }

    #[inline]
    pub const fn goto_entry(entry_stub: usize, kstack_top: usize) -> Self {
        Self {
            ra: entry_stub,
            sp: kstack_top,
            s: [0; 12],
        }
    }
}

#[derive(Clone, Copy)]
pub struct KernelTask {
    pub id: TaskId,
    pub status: TaskStatus,
    pub task_cx: TaskContext,
    pub kernel_stack_top: usize,
    pub entry: KernelTaskEntry,
}
