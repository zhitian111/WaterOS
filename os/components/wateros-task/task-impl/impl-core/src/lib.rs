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
    AddressSpaceRef, CloneFlags, ProcessDescriptor, ProcessId, ProcessState, ProcessTaskDescriptor,
    ProcessTaskRole, TaskClearTid, TaskId, ThreadId,
};
use arch::interrupt::ArchInterruptState;
use base::sync::UniprocessorSafeCell;

mod process;
mod tcb;

pub use api_v0::TaskBootstrap;
pub use process::{ProcessControlBlock, ProcessRegistry};
pub use tcb::TaskControlBlock;

static mut PROCESS_REGISTRY: MaybeUninit<UniprocessorSafeCell<ProcessRegistry>> =
    MaybeUninit::uninit();
static PROCESS_REGISTRY_READY: AtomicBool = AtomicBool::new(false);

fn registry_cell() -> &'static UniprocessorSafeCell<ProcessRegistry> {
    assert!(
        PROCESS_REGISTRY_READY.load(Ordering::Acquire),
        "process registry not initialized"
    );
    unsafe { &*PROCESS_REGISTRY.as_ptr() }
}

/// 初始化进程 registry（幂等）；随后清空全部进程槽位。
/// 初始化进程 registry（幂等，会清空已有记录）。
pub fn init_process_registry() {
    if !PROCESS_REGISTRY_READY.load(Ordering::Acquire) {
        unsafe {
            PROCESS_REGISTRY.write(UniprocessorSafeCell::new(ProcessRegistry::new()));
        }
        PROCESS_REGISTRY_READY.store(true, Ordering::Release);
    }
    registry_cell().exclusive_access().clear();
}

struct ProcessRegistryInterruptGuard {
    state : ArchInterruptState,
}

impl ProcessRegistryInterruptGuard {
    fn new() -> Self {
        let state = arch::interrupt::read_global_interrupt_state()
            .expect("read interrupt state for process registry");
        arch::interrupt::disable_global_interrupt()
            .expect("disable interrupts for process registry");
        Self { state }
    }
}

impl Drop for ProcessRegistryInterruptGuard {
    fn drop(&mut self) {
        arch::interrupt::restore_global_interrupt_state(self.state)
            .expect("restore interrupt state for process registry");
    }
}

/// 在关中断临界区内访问进程 registry。
pub fn with_process_registry<R>(f: impl FnOnce(&mut ProcessRegistry) -> R) -> R {
    let _guard = ProcessRegistryInterruptGuard::new();
    let mut registry = registry_cell().exclusive_access();
    f(&mut registry)
}

/// 查询进程语义快照。
pub fn lookup_process(pid: ProcessId) -> Option<ProcessDescriptor> {
    with_process_registry(|registry| registry.lookup_process(pid))
}

/// 按调度实体查询进程内任务快照。
pub fn lookup_task(task_id: TaskId) -> Option<ProcessTaskDescriptor> {
    with_process_registry(|registry| registry.lookup_task(task_id))
}

/// 返回进程的 leader task id。
pub fn leader_task_for_process(pid: ProcessId) -> Option<TaskId> {
    with_process_registry(|registry| registry.leader_task_for_process(pid))
}

/// 列出进程内全部 task id。
pub fn task_ids_for_process(pid: ProcessId) -> Option<Vec<TaskId>> {
    with_process_registry(|registry| registry.task_ids_for_process(pid))
}

/// 取出已退出的非 leader 线程 task id 列表。
pub fn take_exited_member_tasks(pid: ProcessId) -> Option<Vec<TaskId>> {
    with_process_registry(|registry| registry.take_exited_member_tasks(pid))
}

/// 按用户态 tid 反查内部 task id。
pub fn task_id_for_thread(tid: ThreadId) -> Option<TaskId> {
    with_process_registry(|registry| registry.task_id_for_thread(tid))
}

/// 判断该 task 退出后进程是否无其它存活 task。
pub fn task_exit_would_finish_process(task_id: TaskId) -> Option<bool> {
    with_process_registry(|registry| registry.task_exit_would_finish_process(task_id))
}

/// 查找父进程下一个已退出子进程。
pub fn find_exited_child_process(parent_pid: ProcessId) -> Option<ProcessDescriptor> {
    with_process_registry(|registry| registry.find_exited_child_process(parent_pid))
}

/// 在指定进程组内查找已退出子进程。
pub fn find_exited_child_process_in_pgid(parent_pid: ProcessId,
                                         pgid: ProcessId)
                                         -> Option<ProcessDescriptor> {
    with_process_registry(|registry| {
        registry.find_exited_child_process_in_pgid(parent_pid, pgid)
    })
}

/// 查找父进程下一个可 wait 的 stopped 子进程。
pub fn find_stopped_child_process(parent_pid: ProcessId) -> Option<ProcessDescriptor> {
    with_process_registry(|registry| registry.find_stopped_child_process(parent_pid))
}

/// 判断指定 stopped 子进程是否可被 wait。
pub fn stopped_child_ready_for_wait(parent_pid: ProcessId,
                                    child_pid: ProcessId)
                                    -> Option<ProcessDescriptor> {
    with_process_registry(|registry| {
        registry.stopped_child_ready_for_wait(parent_pid, child_pid)
    })
}

/// 在进程组内查找 stopped 子进程。
pub fn find_stopped_child_process_in_pgid(parent_pid: ProcessId,
                                          pgid: ProcessId)
                                          -> Option<ProcessDescriptor> {
    with_process_registry(|registry| {
        registry.find_stopped_child_process_in_pgid(parent_pid, pgid)
    })
}

/// 查找父进程下一个 continued 子进程。
pub fn find_continued_child_process(parent_pid: ProcessId) -> Option<ProcessDescriptor> {
    with_process_registry(|registry| registry.find_continued_child_process(parent_pid))
}

/// 判断指定 continued 子进程是否可被 wait。
pub fn continued_child_ready_for_wait(parent_pid: ProcessId,
                                        child_pid: ProcessId)
                                        -> Option<ProcessDescriptor> {
    with_process_registry(|registry| {
        registry.continued_child_ready_for_wait(parent_pid, child_pid)
    })
}

/// 在进程组内查找 continued 子进程。
pub fn find_continued_child_process_in_pgid(parent_pid: ProcessId,
                                            pgid: ProcessId)
                                            -> Option<ProcessDescriptor> {
    with_process_registry(|registry| {
        registry.find_continued_child_process_in_pgid(parent_pid, pgid)
    })
}

/// 将进程标为 SIGSTOP 停止态。
pub fn mark_process_stopped(pid: ProcessId, signo: u8) -> bool {
    with_process_registry(|registry| registry.mark_process_stopped(pid, signo))
}

/// 将进程从 stopped 恢复为 running。
pub fn mark_process_continued(pid: ProcessId) -> bool {
    with_process_registry(|registry| registry.mark_process_continued(pid))
}

/// 消费 stop 事件的 wait 可见性。
pub fn consume_stop_wait(pid: ProcessId, nowait: bool) {
    with_process_registry(|registry| registry.consume_stop_wait(pid, nowait))
}

/// 消费 continued 事件的 wait 可见性。
pub fn consume_continued_wait(pid: ProcessId, nowait: bool) {
    with_process_registry(|registry| registry.consume_continued_wait(pid, nowait))
}

/// 父进程是否仍有子进程。
pub fn has_child_process(parent_pid: ProcessId) -> bool {
    with_process_registry(|registry| registry.has_child_process(parent_pid))
}

/// 父进程在指定 pgid 内是否仍有子进程。
pub fn has_child_process_in_pgid(parent_pid: ProcessId, pgid: ProcessId) -> bool {
    with_process_registry(|registry| registry.has_child_process_in_pgid(parent_pid, pgid))
}

/// 为进程创建新会话。
pub fn create_session_for_process(pid: ProcessId) -> Result<(), ()> {
    with_process_registry(|registry| registry.create_session_for_process(pid))
}

/// 查询进程 dumpable 标志。
pub fn process_dumpable(pid: ProcessId) -> Option<bool> {
    with_process_registry(|registry| registry.process_dumpable(pid))
}

/// 设置进程 dumpable 标志。
pub fn set_process_dumpable(pid: ProcessId, dumpable: bool) -> bool {
    with_process_registry(|registry| registry.set_process_dumpable(pid, dumpable))
}

/// 查询 child subreaper 标志。
pub fn process_child_subreaper(pid: ProcessId) -> Option<bool> {
    with_process_registry(|registry| registry.process_child_subreaper(pid))
}

/// 设置 child subreaper 标志。
pub fn set_process_child_subreaper(pid: ProcessId, enabled: bool) -> bool {
    with_process_registry(|registry| registry.set_process_child_subreaper(pid, enabled))
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
pub fn set_task_clear_child_tid(task_id: TaskId, clear_child_tid: Option<TaskClearTid>) -> bool {
    with_process_registry(|registry| registry.set_task_clear_child_tid(task_id, clear_child_tid))
}

/// 读取任务的 clear-child-tid 地址。
pub fn task_clear_child_tid(task_id: TaskId) -> Option<TaskClearTid> {
    with_process_registry(|registry| registry.task_clear_child_tid(task_id))
}

/// 回收已退出进程并返回其快照。
pub fn reap_process(pid: ProcessId) -> Option<ProcessDescriptor> {
    with_process_registry(|registry| registry.reap_process(pid))
}

/// 回收已退出进程并返回关联 task id 列表。
pub fn reap_process_with_tasks(pid: ProcessId) -> Option<(ProcessDescriptor, Vec<TaskId>)> {
    with_process_registry(|registry| registry.reap_process_with_tasks(pid))
}

/// fork 失败时撤销子进程 registry 记录。
pub fn abort_forked_process(child_task_id: TaskId) -> Option<ProcessId> {
    with_process_registry(|registry| registry.abort_forked_process(child_task_id))
}

/// clone 线程失败时从进程表移除子线程。
pub fn abort_cloned_thread(child_task_id: TaskId) -> bool {
    with_process_registry(|registry| registry.abort_cloned_thread(child_task_id))
}

/// 读取进程资源限制。
pub fn get_process_rlimit(pid: ProcessId, resource: usize) -> Option<api_v0::ResourceLimit> {
    with_process_registry(|registry| registry.get_process_rlimit(pid, resource))
}

/// 写入进程资源限制。
pub fn set_process_rlimit(
    pid: ProcessId,
    resource: usize,
    limit: api_v0::ResourceLimit,
) -> Result<(), api_v0::SetResourceLimitError> {
    with_process_registry(|registry| registry.set_process_rlimit(pid, resource, limit))
}

/// 读取进程 nice 值。
pub fn get_process_nice(pid: ProcessId) -> Option<i32> {
    with_process_registry(|registry| registry.get_process_nice(pid))
}

/// 设置进程 nice 值。
pub fn set_process_nice(pid: ProcessId, nice: i32) -> bool {
    with_process_registry(|registry| registry.set_process_nice(pid, nice))
}

/// 读取进程组 id。
pub fn get_process_pgid(pid: ProcessId) -> Option<ProcessId> {
    with_process_registry(|registry| registry.get_process_pgid(pid))
}

/// 设置进程组 id。
pub fn set_process_pgid(pid: ProcessId, pgid: ProcessId) -> bool {
    with_process_registry(|registry| registry.set_process_pgid(pid, pgid))
}

/// 将 nice 写入同一 pgid 下全部进程。
pub fn set_nice_for_pgid(pgid: ProcessId, nice: i32) -> bool {
    with_process_registry(|registry| registry.set_nice_for_pgid(pgid, nice))
}

/// 进程组内最小 nice（最高优先级）。
pub fn min_nice_in_pgid(pgid: ProcessId) -> Option<i32> {
    with_process_registry(|registry| registry.min_nice_in_pgid(pgid))
}

/// 进程 pid 是否仍占 registry 槽位。
pub fn process_exists(pid: ProcessId) -> bool {
    with_process_registry(|registry| registry.process_exists(pid))
}

/// 进程组是否仍有成员进程。
pub fn pgid_has_members(pgid: ProcessId) -> bool {
    with_process_registry(|registry| registry.pgid_has_members(pgid))
}

pub fn process_model_self_test() {
    let mut registry = ProcessRegistry::new();
    let aspace = Some(AddressSpaceRef::new(
        api_v0::AddressSpaceHandle::from_raw(0x1000),
        0x2000,
    ));

    let pid = registry.create_process_for_task(100, None, aspace);
    assert_eq!(pid.raw(), 1);
    let leader = registry.lookup_task(100).expect("leader task must be indexed");
    assert_eq!(leader.task_id, 100);
    assert_eq!(leader.tid.raw(), pid.raw());
    assert_eq!(leader.pid, pid);
    assert_eq!(leader.role, ProcessTaskRole::Leader);
    assert_eq!(registry.lookup_process(pid).unwrap().task_count, 1);
    assert_eq!(registry.lookup_process(pid).unwrap().file_table.unwrap().raw(), 100);
    assert_eq!(registry.lookup_process(pid).unwrap().cwd.unwrap().raw(), 100);

    let task101 = registry
        .add_task_to_process(
            pid,
            101,
            CloneFlags::CLONE_TASK_GROUP | CloneFlags::CLONE_SETTLS,
            0x3000,
            Some(TaskClearTid::new(0x4000)),
        )
        .expect("create process member task");
    let member101 = registry
        .lookup_task(task101)
        .expect("member task must be indexed");
    assert_eq!(member101.task_id, 101);
    assert_eq!(member101.tid.raw(), 2);
    assert_ne!(member101.tid.raw(), member101.pid.raw());
    assert_eq!(registry.task_id_for_thread(member101.tid), Some(101));
    assert_eq!(member101.role, ProcessTaskRole::Member);
    assert_eq!(member101.tls, 0x3000);
    assert_eq!(member101.clear_child_tid.unwrap().user_addr(), 0x4000);
    assert_eq!(registry.lookup_process(pid).unwrap().task_count, 2);
    assert_eq!(registry.task_exit_would_finish_process(100), Some(false));
    assert_eq!(registry.task_exit_would_finish_process(101), Some(false));

    let forked = registry
        .create_process_like_fork(pid, 102, aspace)
        .expect("fork-style process");
    assert_eq!(forked.raw(), 3);
    let forked_leader = registry.lookup_task(102).expect("forked leader");
    assert_eq!(forked_leader.task_id, 102);
    assert_eq!(forked_leader.tid.raw(), forked.raw());
    assert_eq!(forked_leader.pid, forked);
    assert_eq!(registry.lookup_process(forked).unwrap().parent_pid, Some(pid));
    assert!(registry.has_child_process(pid));

    assert!(registry.mark_task_exited(101, 7));
    assert_eq!(registry.task_exit_would_finish_process(100), Some(true));
    assert!(matches!(
        registry.lookup_process(pid).unwrap().state,
        ProcessState::Running
    ));
    assert!(registry.mark_task_exited(100, 9));
    assert!(matches!(
        registry.lookup_process(pid).unwrap().state,
        ProcessState::Exited(9)
    ));
    assert!(registry.set_task_clear_child_tid(100, Some(TaskClearTid::new(0x5000))));
    assert_eq!(registry.task_clear_child_tid(100).unwrap().user_addr(), 0x5000);

    assert!(registry.mark_task_exited(102, 3));
    assert_eq!(registry.find_exited_child_process(pid).unwrap().pid, forked);
    assert!(registry.reap_process(forked).is_some());
    assert!(!registry.has_child_process(pid));
}
