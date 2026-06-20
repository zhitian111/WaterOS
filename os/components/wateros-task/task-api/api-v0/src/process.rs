//! 进程语义层的稳定类型。
//!
//! `TaskId` 表示调度器中的内部可运行实体；`ProcessId` / `ThreadId` 表示用户态
//! 可见的进程与线程身份。

use core::ops::{BitOr, BitOrAssign};

use crate::{AddressSpaceHandle, TaskExitCode, TaskId};

/// 用户态进程 ID；`getpid()` / `waitpid()` / `/proc/<pid>` 使用该值。
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProcessId(pub usize);

impl ProcessId {
    #[inline]
    pub const fn from_raw(raw: usize) -> Self {
        Self(raw)
    }

    #[inline]
    pub const fn raw(self) -> usize {
        self.0
    }
}

/// 用户态线程 ID；`gettid()` / `clone(*TID)` / robust futex owner 使用该值。
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ThreadId(pub usize);

impl ThreadId {
    #[inline]
    pub const fn from_raw(raw: usize) -> Self {
        Self(raw)
    }

    #[inline]
    pub const fn raw(self) -> usize {
        self.0
    }
}

/// 进程内任务组 ID；默认等于进程 leader 的 [`ProcessId`]。
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskGroupId(pub usize);

impl TaskGroupId {
    #[inline]
    pub const fn from_raw(raw: usize) -> Self {
        Self(raw)
    }

    #[inline]
    pub const fn raw(self) -> usize {
        self.0
    }
}

/// Linux `clone(2)` 常用 flags 的语义子集。
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CloneFlags(usize);

impl CloneFlags {
    pub const CLONE_VM: Self = Self(0x0000_0100);
    pub const CLONE_FS: Self = Self(0x0000_0200);
    pub const CLONE_FILES: Self = Self(0x0000_0400);
    pub const CLONE_SIGHAND: Self = Self(0x0000_0800);
    pub const CLONE_THREAD: Self = Self(0x0001_0000);
    pub const CLONE_TASK_GROUP: Self = Self::CLONE_THREAD;
    pub const CLONE_SETTLS: Self = Self(0x0008_0000);
    pub const CLONE_PARENT_SETTID: Self = Self(0x0010_0000);
    pub const CLONE_CHILD_CLEARTID: Self = Self(0x0020_0000);
    pub const CLONE_CHILD_SETTID: Self = Self(0x0100_0000);

    #[inline]
    pub const fn empty() -> Self {
        Self(0)
    }

    #[inline]
    pub const fn from_bits(bits: usize) -> Self {
        Self(bits)
    }

    #[inline]
    pub const fn bits(self) -> usize {
        self.0
    }

    #[inline]
    pub const fn contains(self, flag: Self) -> bool {
        (self.0 & flag.0) == flag.0
    }
}

impl BitOr for CloneFlags {
    type Output = Self;

    #[inline]
    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for CloneFlags {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// 用户地址空间共享资源引用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddressSpaceRef {
    token: AddressSpaceHandle,
    user_aspace_ptr: usize,
}

impl AddressSpaceRef {
    #[inline]
    pub const fn new(token: AddressSpaceHandle, user_aspace_ptr: usize) -> Self {
        Self {
            token,
            user_aspace_ptr,
        }
    }

    #[inline]
    pub const fn token(self) -> AddressSpaceHandle {
        self.token
    }

    #[inline]
    pub const fn user_aspace_ptr(self) -> usize {
        self.user_aspace_ptr
    }
}

/// fd table / cwd / signal handler 等后续资源表的稳定占位句柄。
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResourceHandle(pub usize);

impl ResourceHandle {
    #[inline]
    pub const fn from_raw(raw: usize) -> Self {
        Self(raw)
    }

    #[inline]
    pub const fn raw(self) -> usize {
        self.0
    }
}

pub type FileTableRef = ResourceHandle;
pub type CwdRef = ResourceHandle;
pub type SignalHandlersRef = ResourceHandle;

/// `set_tid_address` / 清理任务退出标记使用的用户地址。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskClearTid {
    user_addr: usize,
}

impl TaskClearTid {
    #[inline]
    pub const fn new(user_addr: usize) -> Self {
        Self { user_addr }
    }

    #[inline]
    pub const fn user_addr(self) -> usize {
        self.user_addr
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessTaskRole {
    Leader,
    Member,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessTaskState {
    Runnable,
    Exited(TaskExitCode),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessState {
    Running,
    Exiting(TaskExitCode),
    Exited(TaskExitCode),
}

/// 对外可见的进程内任务语义快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessTaskDescriptor {
    pub task_id: TaskId,
    pub tid: ThreadId,
    pub pid: ProcessId,
    pub role: ProcessTaskRole,
    pub state: ProcessTaskState,
    pub tls: usize,
    pub clear_child_tid: Option<TaskClearTid>,
}

/// Linux `struct rlimit` 语义子集（`rlim_cur` / `rlim_max`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceLimit {
    pub cur: u64,
    pub max: u64,
}

/// `setrlimit` / `prlimit64` 写入失败原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetResourceLimitError {
    /// `cur > max` 或未知资源号。
    InvalidArgument,
}

/// 对外可见的进程语义快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessDescriptor {
    pub pid: ProcessId,
    pub task_group_id: TaskGroupId,
    pub leader_task_id: TaskId,
    pub parent_pid: Option<ProcessId>,
    pub address_space: Option<AddressSpaceRef>,
    pub file_table: Option<FileTableRef>,
    pub cwd: Option<CwdRef>,
    pub signal_handlers: Option<SignalHandlersRef>,
    pub task_count: usize,
    pub state: ProcessState,
}
