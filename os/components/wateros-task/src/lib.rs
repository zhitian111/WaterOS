//! WaterOS 任务子系统聚合 crate：对上暴露稳定 API，对下组合 **任务数据模型** 与 **调度实现**。
//!
//! ## 职责划分
//!
//! - [`api`]（`wateros-task-api-v0`）：任务 ID、状态、等待句柄、用户任务规格等 **跨层语义类型**；不含调度策略与 per-task 内存布局。
//! - [`scheduler`]（`wateros-task-scheduler`）：**何时运行谁**——就绪队列、阻塞/睡眠/等待、tick 与主动让出、上下文切换入口；通过 `active_impl` 绑定具体算法（如轮转）。
//! - **`impl-core`**（`wateros-task-impl-core`，feature `impl-core`）：**单个任务长什么样**——`TaskControlBlock`、内核/用户栈、trap 现场与用户态镜像的装配；供调度器实现持有并驱动切换，本 crate 通过 [`crate::active_impl::TaskBootstrap`] 等再导出给 arch 入口。
//!
//! 本文件中的 `spawn`/`yield`/`wait` 等函数是对 `scheduler` 的薄封装；[`trap_runtime`] 提供具名 Rust 入口，
//! `runtime` 提供与汇编/switch 约定的 `extern "C"` 符号；组合层 `trap_handler::init` 注册 `arch-api::kernel_trap` 后由 `trap_entry_rust` 转入。
//!
//! ## 后续替换点
//!
//! 更换调度算法时改 `task-scheduler` 的 `active_impl`；更换 TCB/栈布局时改 `impl-core`。二者边界应保持：**调度器不定义 TCB 字段布局，impl-core 不决定全局就绪顺序**。

#![no_std]

mod runtime;
pub mod trap_runtime;

pub mod api {
    pub use api_v0::*;
}

pub mod scheduler {
    pub use scheduler::*;
}

#[cfg(feature = "impl-core")]
pub use impl_core as active_impl;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaitQueue {
    id: WaitQueueId,
}

impl WaitQueue {
    /// 创建一个新的等待队列句柄。
    #[inline]
    pub fn new() -> Self {
        Self {
            id: scheduler::allocate_wait_queue(),
        }
    }

    /// 返回该等待队列对应的内部编号。
    #[inline]
    pub const fn id(&self) -> WaitQueueId {
        self.id
    }

    /// 返回该等待队列对应的通用等待句柄。
    #[inline]
    pub const fn wait_handle(&self) -> TaskWaitHandle {
        TaskWaitHandle::for_wait_queue(self.id)
    }

    /// 让当前任务在该等待队列上休眠，直到被显式唤醒。
    #[inline]
    pub fn wait_current(&self) {
        scheduler::wait_current(self.wait_handle());
    }

    /// 让当前任务在该等待队列上等待，超时后返回等待结果。
    #[inline]
    pub fn wait_current_for_ticks(&self, timeout_ticks: TaskTick) -> TaskWaitResult {
        scheduler::wait_current_timeout(self.wait_handle(), timeout_ticks)
    }

    /// 唤醒该等待队列中的一个任务，并返回被唤醒的任务号。
    #[inline]
    pub fn wake_one(&self) -> Option<TaskId> {
        scheduler::wake_one_in_wait_queue(self.id)
    }

    /// 唤醒该等待队列中的全部任务，并返回实际唤醒数量。
    #[inline]
    pub fn wake_all(&self) -> usize {
        scheduler::wake_all_in_wait_queue(self.id)
    }
}

pub use api_v0::{
    AddressSpaceHandle, ExitedTask, KernelTaskEntry, TaskBlockReason, TaskExitCode, TaskId,
    TaskKind, TaskSnapshot, TaskState, TaskTick, TaskTrapSnapshot, TaskWaitHandle, TaskWaitResult,
    TaskWaitTarget, UserImageInfo, UserTaskEntryPc, UserTaskResources, UserTaskSpec, WaitQueueId,
    IDLE_TASK_ID,
};

/// 初始化任务系统和底层调度器状态。
#[inline]
pub fn init() {
    scheduler::init();
}

/// 在全局内核页表 `satp` 就绪后注册，供 trap 在返回内核态时写回。
#[inline]
pub fn init_kernel_trap_satp(v: usize) {
    crate::trap_runtime::init_kernel_trap_satp(v);
}

/// 创建一个新的内核任务，并返回分配到的任务号。
#[inline]
pub fn spawn_kernel_task(entry: KernelTaskEntry, arg: usize) -> TaskId {
    scheduler::spawn_kernel_task(entry, arg)
}

/// 按给定规格创建一个新的用户任务，并返回分配到的任务号。
#[inline]
pub fn spawn_user_task_spec(spec: UserTaskSpec) -> TaskId {
    scheduler::spawn_user_task_spec(spec)
}

/// 创建一个新的最小用户任务骨架，并返回分配到的任务号。
#[inline]
pub fn spawn_user_task(entry_pc: UserTaskEntryPc) -> TaskId {
    spawn_user_task_spec(UserTaskSpec::new(entry_pc))
}

/// 启动调度器并切入第一批可运行任务。
#[inline]
pub fn run_first_task() -> ! {
    scheduler::run_first_task()
}

/// 让当前任务主动让出 CPU。
#[inline]
pub fn yield_now() {
    scheduler::suspend_current_and_run_next();
}

/// 通知任务系统发生了一次时钟 tick。
#[inline]
pub fn schedule_tick() {
    scheduler::schedule_tick();
}

/// 以指定阻塞原因挂起当前任务。
#[inline]
pub fn block_current(reason: TaskBlockReason) {
    scheduler::block_current(reason);
}

/// 让当前任务等待指定的阻塞对象。
#[inline]
pub fn wait_on(wait_handle: TaskWaitHandle) {
    scheduler::wait_current(wait_handle);
}

/// 让当前任务等待指定的阻塞对象，并带一个超时。
#[inline]
pub fn wait_on_for_ticks(wait_handle: TaskWaitHandle, timeout_ticks: TaskTick) -> TaskWaitResult {
    scheduler::wait_current_timeout(wait_handle, timeout_ticks)
}

/// 返回“等待指定任务退出”的通用等待句柄。
#[inline]
pub const fn task_exit_wait_handle(task_id: TaskId) -> TaskWaitHandle {
    TaskWaitHandle::for_task_exit(task_id)
}

/// 让当前任务等待指定任务退出。
#[inline]
pub fn wait_for_task_exit(task_id: TaskId) {
    wait_on(task_exit_wait_handle(task_id));
}

/// 让当前任务等待指定任务退出，并带一个超时。
#[inline]
pub fn wait_for_task_exit_for_ticks(task_id: TaskId, timeout_ticks: TaskTick) -> TaskWaitResult {
    wait_on_for_ticks(
        task_exit_wait_handle(task_id),
        timeout_ticks,
    )
}

/// 让当前任务睡眠指定数量的 tick。
#[inline]
pub fn sleep_for_ticks(ticks: TaskTick) {
    scheduler::sleep_current_for_ticks(ticks);
}

/// 尝试唤醒指定任务。
#[inline]
pub fn wake_task(task_id: TaskId) -> bool {
    scheduler::wake_task(task_id)
}

/// 回收指定已退出任务的信息。
#[inline]
pub fn reap_exited_task(task_id: TaskId) -> Option<ExitedTask> {
    scheduler::reap_exited_task(task_id)
}

/// 回收一个任意已退出任务的信息。
#[inline]
pub fn reap_one_exited_task() -> Option<ExitedTask> {
    scheduler::reap_one_exited_task()
}

/// 让当前任务以给定退出码结束运行。
#[inline]
pub fn exit_current(exit_code: TaskExitCode) -> ! {
    scheduler::exit_current(exit_code)
}

/// 返回当前正在运行任务的任务号。
#[inline]
pub fn current_task_id() -> Option<TaskId> {
    scheduler::current_task_id()
}

/// 返回当前正在运行任务的稳定快照。
#[inline]
pub fn current_task_snapshot() -> Option<TaskSnapshot> {
    scheduler::current_task_snapshot()
}
