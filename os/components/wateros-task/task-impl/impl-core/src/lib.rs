//! 任务 **实现内核**：每个任务的控制块与任务类型专属资源，供调度器实现与 arch
//! 入口共同使用。
//!
//! - **本 crate**：[`TaskControlBlock`] 持有任务通用元数据与 [`TaskInner`]
//!   （区分 Idle / Kernel / User），内核栈与用户栈由对应的任务类型管理。
//! - **`wateros-task-scheduler`**：**组装并驱动** 多个 TCB——创建任务时调用本
//!   crate 类型完成初始化，在 `schedule`/`wait`
//!   等路径上更新状态并触发上下文切换。

#![no_std]
#![allow(static_mut_refs)]

extern crate alloc;

use alloc::vec::Vec;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};

use api_v0::{
    ProcessCaps, ProcessId, ProcessResult, ProcessSnapshot, ProcessTaskSnapshot, TaskClearTid,
    TaskId, ThreadId,
};
use arch::interrupt::ArchInterruptState;
use base::sync::MultiprocessorSafeCell;

mod process;
mod tcb;

pub use api_v0::TaskBootstrap;
pub use process::{ParentDeathNotification, ProcessControlBlock, ProcessRegistry};
pub use tcb::TaskControlBlock;

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[task/impl-core] self_test begin");
    assert!(core::mem::size_of::<TaskControlBlock>() > 0);
    log::info!("[task/impl-core] self_test complete");
}

static mut PROCESS_REGISTRY : MaybeUninit<MultiprocessorSafeCell<ProcessRegistry>> =
    MaybeUninit::uninit();
static PROCESS_REGISTRY_READY : AtomicBool = AtomicBool::new(false);

fn registry_cell() -> &'static MultiprocessorSafeCell<ProcessRegistry> {
    assert!(PROCESS_REGISTRY_READY.load(Ordering::Acquire),
            "process registry not initialized");
    unsafe { &*PROCESS_REGISTRY.as_ptr() }
}

/// 初始化进程 registry（幂等）；随后清空全部进程槽位。
/// 初始化进程 registry（幂等，会清空已有记录）。
pub fn init_process_registry() {
    if !PROCESS_REGISTRY_READY.load(Ordering::Acquire) {
        unsafe {
            PROCESS_REGISTRY.write(MultiprocessorSafeCell::new(ProcessRegistry::new()));
        }
        PROCESS_REGISTRY_READY.store(true, Ordering::Release);
    }
    registry_cell().exclusive_access()
                   .clear();
}

struct ProcessRegistryInterruptGuard {
    state : ArchInterruptState,
}

impl ProcessRegistryInterruptGuard {
    fn new() -> Self {
        let state = arch::interrupt::read_global_interrupt_state().expect("read interrupt state \
                                                                           for process registry");
        arch::interrupt::disable_global_interrupt().expect("disable interrupts for process \
                                                            registry");
        Self { state }
    }
}

impl Drop for ProcessRegistryInterruptGuard {
    fn drop(&mut self) {
        arch::interrupt::restore_global_interrupt_state(self.state).expect("restore interrupt \
                                                                            state for process \
                                                                            registry");
    }
}

/// 在关中断临界区内访问进程 registry。
pub fn with_process_registry<R>(f : impl FnOnce(&mut ProcessRegistry) -> R) -> R {
    let _guard = ProcessRegistryInterruptGuard::new();
    let cpu = arch::cpu::current_cpu_id().raw();
    let cell = registry_cell();
    let object = cell as *const _ as usize;
    let mut registry = if debug::ENABLED {
        if let Some(registry) = cell.try_lock() {
            registry
        } else {
            debug::lock_wait(cpu,
                             0,
                             debug::NO_TASK,
                             debug::DebugLockKind::ProcessRegistry,
                             object);
            cell.exclusive_access()
        }
    } else {
        cell.exclusive_access()
    };
    debug::lock_acquired(cpu,
                         debug::DebugLockKind::ProcessRegistry,
                         object);
    let result = f(&mut registry);
    drop(registry);
    debug::lock_released(cpu,
                         debug::DebugLockKind::ProcessRegistry,
                         object);
    result
}

/// 查询进程语义快照。
pub fn process_snapshot(pid : ProcessId) -> Option<ProcessSnapshot> {
    with_process_registry(|registry| registry.process_snapshot(pid))
}

/// 按调度实体查询进程内任务快照。
pub fn process_task_snapshot(task_id : TaskId) -> Option<ProcessTaskSnapshot> {
    with_process_registry(|registry| registry.process_task_snapshot(task_id))
}

/// 按调度实体查询进程及其父进程标识，避免为标识类 syscall 构造完整快照。
pub fn process_identity_for_task(task_id : TaskId) -> Option<(ProcessId, Option<ProcessId>)> {
    with_process_registry(|registry| registry.process_identity_for_task(task_id))
}

/// 返回进程的 leader task id。
pub fn leader_task_for_process(pid : ProcessId) -> Option<TaskId> {
    with_process_registry(|registry| registry.leader_task_for_process(pid))
}

/// 返回指定进程的全部直接子进程 pid。
pub fn collect_child_pids(pid : ProcessId) -> Vec<ProcessId> {
    with_process_registry(|registry| registry.collect_child_pids(pid))
}

/// 列出进程内全部 task id。
pub fn task_ids_for_process(pid : ProcessId) -> Option<Vec<TaskId>> {
    with_process_registry(|registry| registry.task_ids_for_process(pid))
}

/// 取出已退出的非 leader 线程 task id 列表。
pub fn take_exited_member_tasks(pid : ProcessId) -> Option<Vec<TaskId>> {
    with_process_registry(|registry| registry.take_exited_member_tasks(pid))
}

/// 按用户态 tid 反查内部 task id。
pub fn task_id_for_thread(tid : ThreadId) -> Option<TaskId> {
    with_process_registry(|registry| registry.task_id_for_thread(tid))
}

/// 查找父进程下一个已退出子进程。
pub fn find_exited_child_process(parent_pid : ProcessId) -> Option<ProcessSnapshot> {
    with_process_registry(|registry| registry.find_exited_child_process(parent_pid))
}

/// 在指定进程组内查找已退出子进程。
pub fn find_exited_child_process_in_pgid(parent_pid : ProcessId,
                                         pgid : ProcessId)
                                         -> Option<ProcessSnapshot> {
    with_process_registry(|registry| registry.find_exited_child_process_in_pgid(parent_pid, pgid))
}

/// 查找父进程下一个可 wait 的 stopped 子进程。
pub fn find_stopped_child_process(parent_pid : ProcessId) -> Option<ProcessSnapshot> {
    with_process_registry(|registry| registry.find_stopped_child_process(parent_pid))
}

/// 判断指定 stopped 子进程是否可被 wait。
pub fn stopped_child_ready_for_wait(parent_pid : ProcessId,
                                    child_pid : ProcessId)
                                    -> Option<ProcessSnapshot> {
    with_process_registry(|registry| registry.stopped_child_ready_for_wait(parent_pid, child_pid))
}

/// 在进程组内查找 stopped 子进程。
pub fn find_stopped_child_process_in_pgid(parent_pid : ProcessId,
                                          pgid : ProcessId)
                                          -> Option<ProcessSnapshot> {
    with_process_registry(|registry| registry.find_stopped_child_process_in_pgid(parent_pid, pgid))
}

/// 查找父进程下一个 continued 子进程。
pub fn find_continued_child_process(parent_pid : ProcessId) -> Option<ProcessSnapshot> {
    with_process_registry(|registry| registry.find_continued_child_process(parent_pid))
}

/// 判断指定 continued 子进程是否可被 wait。
pub fn continued_child_ready_for_wait(parent_pid : ProcessId,
                                      child_pid : ProcessId)
                                      -> Option<ProcessSnapshot> {
    with_process_registry(|registry| registry.continued_child_ready_for_wait(parent_pid, child_pid))
}

/// 在进程组内查找 continued 子进程。
pub fn find_continued_child_process_in_pgid(parent_pid : ProcessId,
                                            pgid : ProcessId)
                                            -> Option<ProcessSnapshot> {
    with_process_registry(|registry| {
        registry.find_continued_child_process_in_pgid(parent_pid, pgid)
    })
}

/// 将进程标为 SIGSTOP 停止态。
pub fn mark_process_stopped(pid : ProcessId, signo : u8) -> ProcessResult<()> {
    with_process_registry(|registry| registry.mark_process_stopped(pid, signo))
}

/// 将进程从 stopped 恢复为 running。
pub fn mark_process_continued(pid : ProcessId) -> ProcessResult<()> {
    with_process_registry(|registry| registry.mark_process_continued(pid))
}

/// 消费 stop 事件的 wait 可见性。
pub fn consume_stop_wait(pid : ProcessId, nowait : bool) {
    with_process_registry(|registry| registry.consume_stop_wait(pid, nowait))
}

/// 消费 continued 事件的 wait 可见性。
pub fn consume_continued_wait(pid : ProcessId, nowait : bool) {
    with_process_registry(|registry| registry.consume_continued_wait(pid, nowait))
}

/// 父进程是否仍有子进程。
pub fn has_child_process(parent_pid : ProcessId) -> bool {
    with_process_registry(|registry| registry.has_child_process(parent_pid))
}

/// 父进程在指定 pgid 内是否仍有子进程。
pub fn has_child_process_in_pgid(parent_pid : ProcessId, pgid : ProcessId) -> bool {
    with_process_registry(|registry| registry.has_child_process_in_pgid(parent_pid, pgid))
}

/// 为进程创建新会话。
pub fn create_session_for_process(pid : ProcessId) -> ProcessResult<()> {
    with_process_registry(|registry| registry.create_session_for_process(pid))
}

/// 查询进程 dumpable 标志。
pub fn process_dumpable(pid : ProcessId) -> Option<bool> {
    with_process_registry(|registry| registry.process_dumpable(pid))
}

/// 设置进程 dumpable 标志。
pub fn set_process_dumpable(pid : ProcessId, dumpable : bool) -> ProcessResult<()> {
    with_process_registry(|registry| registry.set_process_dumpable(pid, dumpable))
}

/// 查询 child subreaper 标志。
pub fn process_child_subreaper(pid : ProcessId) -> Option<bool> {
    with_process_registry(|registry| registry.process_child_subreaper(pid))
}

/// 设置 child subreaper 标志。
pub fn set_process_child_subreaper(pid : ProcessId, enabled : bool) -> ProcessResult<()> {
    with_process_registry(|registry| registry.set_process_child_subreaper(pid, enabled))
}

/// 查询进程 capability 三集合。
pub fn process_caps(pid : ProcessId) -> Option<ProcessCaps> {
    with_process_registry(|registry| registry.process_caps(pid))
}

/// 设置进程 capability 三集合。
pub fn set_process_caps(pid : ProcessId, caps : ProcessCaps) -> ProcessResult<()> {
    with_process_registry(|registry| registry.set_process_caps(pid, caps))
}

/// 查询进程 KEEPCAPS 标志（PR_GET_KEEPCAPS）。
pub fn process_keep_caps(pid : ProcessId) -> Option<bool> {
    with_process_registry(|registry| registry.process_keep_caps(pid))
}

/// 设置进程 KEEPCAPS 标志（PR_SET_KEEPCAPS）。
pub fn set_process_keep_caps(pid : ProcessId, enabled : bool) -> ProcessResult<()> {
    with_process_registry(|registry| registry.set_process_keep_caps(pid, enabled))
}

/// 列出 registry 中全部进程 pid。
pub fn all_process_pids() -> Vec<ProcessId> {
    with_process_registry(|registry| registry.all_process_pids())
}

/// 列出尚未 reap 的已退出进程 pid。
pub fn collect_exited_process_pids() -> Vec<ProcessId> {
    with_process_registry(|registry| registry.collect_exited_process_pids())
}

/// 更新任务的 clear-child-tid 地址。
pub fn set_task_clear_child_tid(task_id : TaskId,
                                clear_child_tid : Option<TaskClearTid>)
                                -> ProcessResult<()> {
    with_process_registry(|registry| registry.set_task_clear_child_tid(task_id, clear_child_tid))
}

/// 读取任务的 clear-child-tid 地址。
pub fn task_clear_child_tid(task_id : TaskId) -> Option<TaskClearTid> {
    with_process_registry(|registry| registry.task_clear_child_tid(task_id))
}

/// 回收已退出进程并返回其快照。
pub fn reap_process(pid : ProcessId) -> Option<ProcessSnapshot> {
    let retired = with_process_registry(|registry| registry.detach_exited_process(pid))?;
    Some(retired.cleanup().0)
}

/// 回收已退出进程并返回关联 task id 列表。
pub fn reap_process_with_tasks(pid : ProcessId) -> Option<(ProcessSnapshot, Vec<TaskId>)> {
    let retired = with_process_registry(|registry| registry.detach_exited_process(pid))?;
    Some(retired.cleanup())
}

/// fork 失败时撤销子进程 registry 记录。
pub fn abort_forked_process(child_task_id : TaskId) -> ProcessResult<ProcessId> {
    let (pid, retired) =
        with_process_registry(|registry| registry.detach_aborted_fork(child_task_id))?;
    let _ = retired.cleanup();
    Ok(pid)
}

/// clone 线程失败时从进程表移除子线程。
pub fn abort_cloned_thread(child_task_id : TaskId) -> ProcessResult<()> {
    with_process_registry(|registry| registry.abort_cloned_thread(child_task_id))
}

/// 读取进程资源限制。
pub fn get_process_rlimit(pid : ProcessId, resource : usize) -> Option<api_v0::ResourceLimit> {
    with_process_registry(|registry| registry.get_process_rlimit(pid, resource))
}

/// 写入进程资源限制。
pub fn set_process_rlimit(pid : ProcessId,
                          resource : usize,
                          limit : api_v0::ResourceLimit)
                          -> ProcessResult<()> {
    with_process_registry(|registry| registry.set_process_rlimit(pid, resource, limit))
}

/// 读取进程 umask。
pub fn get_process_umask(pid : ProcessId) -> Option<u32> {
    with_process_registry(|registry| registry.get_process_umask(pid))
}

/// 设置进程 umask。
pub fn set_process_umask(pid : ProcessId, mask : u32) -> ProcessResult<()> {
    with_process_registry(|registry| registry.set_process_umask(pid, mask))
}

/// 读取进程 parent-death-signal。
pub fn get_parent_death_signal(pid : ProcessId) -> Option<i32> {
    with_process_registry(|registry| registry.get_parent_death_signal(pid))
}

/// 设置进程 parent-death-signal。
pub fn set_parent_death_signal(pid : ProcessId, sig : i32) -> ProcessResult<()> {
    with_process_registry(|registry| registry.set_parent_death_signal(pid, sig))
}

/// 读取线程名（`comm`）。
pub fn get_thread_comm(task_id : TaskId) -> Option<[u8; 16]> {
    with_process_registry(|registry| registry.get_thread_comm(task_id))
}

/// 设置线程名（`comm`）。
pub fn set_thread_comm(task_id : TaskId, comm : [u8; 16]) -> ProcessResult<()> {
    with_process_registry(|registry| registry.set_thread_comm(task_id, comm))
}

/// 读取进程组 id。
pub fn get_process_pgid(pid : ProcessId) -> Option<ProcessId> {
    with_process_registry(|registry| registry.get_process_pgid(pid))
}

/// 设置进程组 id。
pub fn set_process_pgid(pid : ProcessId, pgid : ProcessId) -> ProcessResult<()> {
    with_process_registry(|registry| registry.set_process_pgid(pid, pgid))
}

/// 进程 pid 是否仍占 registry 槽位。
pub fn process_exists(pid : ProcessId) -> bool {
    with_process_registry(|registry| registry.process_exists(pid))
}

/// 进程组是否仍有成员进程。
pub fn pgid_has_members(pgid : ProcessId) -> bool {
    with_process_registry(|registry| registry.pgid_has_members(pgid))
}

/// 原子快照指定进程组中的全部进程 pid。
pub fn process_pids_in_pgid(pgid : ProcessId) -> Vec<ProcessId> {
    with_process_registry(|registry| registry.process_pids_in_pgid(pgid))
}
