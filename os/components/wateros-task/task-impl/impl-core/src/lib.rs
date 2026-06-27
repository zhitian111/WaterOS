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

pub fn with_process_registry<R>(f: impl FnOnce(&mut ProcessRegistry) -> R) -> R {
    let _guard = ProcessRegistryInterruptGuard::new();
    let mut registry = registry_cell().exclusive_access();
    f(&mut registry)
}

pub fn lookup_process(pid: ProcessId) -> Option<ProcessDescriptor> {
    with_process_registry(|registry| registry.lookup_process(pid))
}

pub fn lookup_task(task_id: TaskId) -> Option<ProcessTaskDescriptor> {
    with_process_registry(|registry| registry.lookup_task(task_id))
}

pub fn leader_task_for_process(pid: ProcessId) -> Option<TaskId> {
    with_process_registry(|registry| registry.leader_task_for_process(pid))
}

pub fn task_ids_for_process(pid: ProcessId) -> Option<Vec<TaskId>> {
    with_process_registry(|registry| registry.task_ids_for_process(pid))
}

pub fn take_exited_member_tasks(pid: ProcessId) -> Option<Vec<TaskId>> {
    with_process_registry(|registry| registry.take_exited_member_tasks(pid))
}

pub fn task_id_for_thread(tid: ThreadId) -> Option<TaskId> {
    with_process_registry(|registry| registry.task_id_for_thread(tid))
}

pub fn task_exit_would_finish_process(task_id: TaskId) -> Option<bool> {
    with_process_registry(|registry| registry.task_exit_would_finish_process(task_id))
}

pub fn find_exited_child_process(parent_pid: ProcessId) -> Option<ProcessDescriptor> {
    with_process_registry(|registry| registry.find_exited_child_process(parent_pid))
}

pub fn find_exited_child_process_by_pid(
    parent_pid: ProcessId,
    child_pid: ProcessId,
) -> Option<ProcessDescriptor> {
    with_process_registry(|registry| {
        registry.find_exited_child_process_by_pid(parent_pid, child_pid)
    })
}

pub fn mark_wait_status_delivered(pid: ProcessId) -> bool {
    with_process_registry(|registry| registry.mark_wait_status_delivered(pid))
}

pub fn process_wait_status_delivered(pid: ProcessId) -> bool {
    with_process_registry(|registry| registry.process_wait_status_delivered(pid))
}

pub fn exit_code_for_process(pid: ProcessId) -> Option<isize> {
    with_process_registry(|registry| registry.exit_code_for_process(pid))
}

pub fn finalize_wait_delivered_process(pid: ProcessId) -> bool {
    with_process_registry(|registry| registry.finalize_wait_delivered_process(pid))
}

pub fn has_child_process(parent_pid: ProcessId) -> bool {
    with_process_registry(|registry| registry.has_child_process(parent_pid))
}

pub fn all_process_pids() -> Vec<ProcessId> {
    with_process_registry(|registry| registry.all_process_pids())
}

pub fn collect_exited_process_pids() -> Vec<ProcessId> {
    with_process_registry(|registry| registry.collect_exited_process_pids())
}

pub fn set_task_clear_child_tid(task_id: TaskId, clear_child_tid: Option<TaskClearTid>) -> bool {
    with_process_registry(|registry| registry.set_task_clear_child_tid(task_id, clear_child_tid))
}

pub fn task_clear_child_tid(task_id: TaskId) -> Option<TaskClearTid> {
    with_process_registry(|registry| registry.task_clear_child_tid(task_id))
}

pub fn reap_process(pid: ProcessId) -> Option<ProcessDescriptor> {
    with_process_registry(|registry| registry.reap_process(pid))
}

pub fn reap_process_with_tasks(pid: ProcessId) -> Option<(ProcessDescriptor, Vec<TaskId>)> {
    with_process_registry(|registry| registry.reap_process_with_tasks(pid))
}

pub fn abort_forked_process(child_task_id: TaskId) -> Option<ProcessId> {
    with_process_registry(|registry| registry.abort_forked_process(child_task_id))
}

pub fn abort_cloned_thread(child_task_id: TaskId) -> bool {
    with_process_registry(|registry| registry.abort_cloned_thread(child_task_id))
}

pub fn get_process_rlimit(pid: ProcessId, resource: usize) -> Option<api_v0::ResourceLimit> {
    with_process_registry(|registry| registry.get_process_rlimit(pid, resource))
}

pub fn set_process_rlimit(
    pid: ProcessId,
    resource: usize,
    limit: api_v0::ResourceLimit,
) -> Result<(), api_v0::SetResourceLimitError> {
    with_process_registry(|registry| registry.set_process_rlimit(pid, resource, limit))
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
