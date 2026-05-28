//! 进程语义 registry。
//!
//! 这里不参与调度决策；调度器仍只处理 `TaskId`。在 WaterOS 当前模型里，
//! `ProcessRegistry` 只维护 process 对共享资源的归属，以及 process 下有哪些 task。

extern crate alloc;

use alloc::vec::Vec;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};

use api_v0::{
    AddressSpaceRef, CloneFlags, CwdRef, FileTableRef, ProcessDescriptor, ProcessId,
    ProcessState, ProcessTaskDescriptor, ProcessTaskRole, ProcessTaskState, SignalHandlersRef,
    TaskClearTid, TaskExitCode, TaskGroupId, TaskId,
};
use base::sync::UniprocessorSafeCell;

#[derive(Clone, Debug)]
struct ProcessTask {
    task_id: TaskId,
    state: ProcessTaskState,
    tls: usize,
    clear_child_tid: Option<TaskClearTid>,
}

impl ProcessTask {
    fn descriptor(&self, pid: ProcessId, leader_task_id: TaskId) -> ProcessTaskDescriptor {
        ProcessTaskDescriptor {
            task_id: self.task_id,
            pid,
            role: if self.task_id == leader_task_id {
                ProcessTaskRole::Leader
            } else {
                ProcessTaskRole::Member
            },
            state: self.state,
            tls: self.tls,
            clear_child_tid: self.clear_child_tid,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProcessControlBlock {
    pid: ProcessId,
    task_group_id: TaskGroupId,
    leader_task_id: TaskId,
    parent_pid: Option<ProcessId>,
    address_space: Option<AddressSpaceRef>,
    file_table: Option<FileTableRef>,
    cwd: Option<CwdRef>,
    signal_handlers: Option<SignalHandlersRef>,
    tasks: Vec<ProcessTask>,
    state: ProcessState,
}

impl ProcessControlBlock {
    fn descriptor(&self) -> ProcessDescriptor {
        ProcessDescriptor {
            pid: self.pid,
            task_group_id: self.task_group_id,
            leader_task_id: self.leader_task_id,
            parent_pid: self.parent_pid,
            address_space: self.address_space,
            file_table: self.file_table,
            cwd: self.cwd,
            signal_handlers: self.signal_handlers,
            task_count: self.tasks.len(),
            state: self.state,
        }
    }

    fn task_descriptor(&self, task_id: TaskId) -> Option<ProcessTaskDescriptor> {
        self.tasks
            .iter()
            .find(|task| task.task_id == task_id)
            .map(|task| task.descriptor(self.pid, self.leader_task_id))
    }
}

/// 单核 bring-up 用进程注册表。
pub struct ProcessRegistry {
    processes: Vec<Option<ProcessControlBlock>>,
    next_pid: usize,
}

impl ProcessRegistry {
    pub fn new() -> Self {
        Self {
            processes: Vec::new(),
            next_pid: 1,
        }
    }

    pub fn clear(&mut self) {
        self.processes.clear();
        self.next_pid = 1;
    }

    pub fn create_process_for_task(
        &mut self,
        task_id: TaskId,
        parent_pid: Option<ProcessId>,
        address_space: Option<AddressSpaceRef>,
    ) -> ProcessId {
        assert!(
            self.lookup_task(task_id).is_none(),
            "task {} is already registered in a process",
            task_id
        );
        let pid = self.alloc_pid();
        let process = ProcessControlBlock {
            pid,
            task_group_id: TaskGroupId::from_raw(pid.raw()),
            leader_task_id: task_id,
            parent_pid,
            address_space,
            file_table: None,
            cwd: None,
            signal_handlers: None,
            tasks: alloc::vec![ProcessTask {
                task_id,
                state: ProcessTaskState::Runnable,
                tls: 0,
                clear_child_tid: None,
            }],
            state: ProcessState::Running,
        };
        self.insert_process(process);
        pid
    }

    pub fn create_process_like_fork(
        &mut self,
        parent_pid: ProcessId,
        child_task_id: TaskId,
        address_space: Option<AddressSpaceRef>,
    ) -> Option<ProcessId> {
        self.lookup_process(parent_pid)?;
        Some(self.create_process_for_task(child_task_id, Some(parent_pid), address_space))
    }

    pub fn add_task_to_process(
        &mut self,
        pid: ProcessId,
        task_id: TaskId,
        clone_flags: CloneFlags,
        tls: usize,
        clear_child_tid: Option<TaskClearTid>,
    ) -> Option<TaskId> {
        if self.lookup_task(task_id).is_some() {
            return None;
        }
        let process = self.process_mut(pid)?;
        process.tasks.push(ProcessTask {
            task_id,
            state: ProcessTaskState::Runnable,
            tls: if clone_flags.contains(CloneFlags::CLONE_SETTLS) {
                tls
            } else {
                0
            },
            clear_child_tid,
        });
        Some(task_id)
    }

    pub fn mark_task_exited(&mut self, task_id: TaskId, exit_code: TaskExitCode) -> bool {
        for process in self
            .processes
            .iter_mut()
            .filter_map(|slot| slot.as_mut())
        {
            let Some(task) = process.tasks.iter_mut().find(|task| task.task_id == task_id) else {
                continue;
            };
            task.state = ProcessTaskState::Exited(exit_code);
            if process
                .tasks
                .iter()
                .all(|task| matches!(task.state, ProcessTaskState::Exited(_)))
            {
                process.state = ProcessState::Exited(exit_code);
            }
            return true;
        }
        false
    }

    pub fn mark_process_exiting(&mut self, pid: ProcessId, exit_code: TaskExitCode) -> bool {
        let Some(process) = self.process_mut(pid) else {
            return false;
        };
        process.state = ProcessState::Exited(exit_code);
        for task in &mut process.tasks {
            task.state = ProcessTaskState::Exited(exit_code);
        }
        true
    }

    pub fn lookup_process(&self, pid: ProcessId) -> Option<ProcessDescriptor> {
        self.processes
            .get(pid.raw())
            .and_then(|slot| slot.as_ref())
            .map(ProcessControlBlock::descriptor)
    }

    pub fn lookup_task(&self, task_id: TaskId) -> Option<ProcessTaskDescriptor> {
        self.processes
            .iter()
            .filter_map(|slot| slot.as_ref())
            .find_map(|process| process.task_descriptor(task_id))
    }

    fn alloc_pid(&mut self) -> ProcessId {
        let pid = ProcessId::from_raw(self.next_pid);
        self.next_pid += 1;
        pid
    }

    fn insert_process(&mut self, process: ProcessControlBlock) {
        let idx = process.pid.raw();
        if self.processes.len() <= idx {
            self.processes.resize_with(idx + 1, || None);
        }
        assert!(
            self.processes[idx].is_none(),
            "process slot {} already occupied",
            idx
        );
        self.processes[idx] = Some(process);
    }

    fn process_mut(&mut self, pid: ProcessId) -> Option<&mut ProcessControlBlock> {
        self.processes
            .get_mut(pid.raw())
            .and_then(|slot| slot.as_mut())
    }
}

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

pub fn with_process_registry<R>(f: impl FnOnce(&mut ProcessRegistry) -> R) -> R {
    let mut registry = registry_cell().exclusive_access();
    f(&mut registry)
}

pub fn lookup_process(pid: ProcessId) -> Option<ProcessDescriptor> {
    with_process_registry(|registry| registry.lookup_process(pid))
}

pub fn lookup_task(task_id: TaskId) -> Option<ProcessTaskDescriptor> {
    with_process_registry(|registry| registry.lookup_task(task_id))
}

pub fn self_test() {
    let mut registry = ProcessRegistry::new();
    let aspace = Some(AddressSpaceRef::new(
        api_v0::AddressSpaceHandle::from_raw(0x1000),
        0x2000,
    ));

    let pid = registry.create_process_for_task(100, None, aspace);
    let leader = registry.lookup_task(100).expect("leader task must be indexed");
    assert_eq!(leader.task_id, 100);
    assert_eq!(leader.pid, pid);
    assert_eq!(leader.role, ProcessTaskRole::Leader);
    assert_eq!(registry.lookup_process(pid).unwrap().task_count, 1);

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
    assert_eq!(member101.role, ProcessTaskRole::Member);
    assert_eq!(member101.tls, 0x3000);
    assert_eq!(member101.clear_child_tid.unwrap().user_addr(), 0x4000);
    assert_eq!(registry.lookup_process(pid).unwrap().task_count, 2);

    let forked = registry
        .create_process_like_fork(pid, 102, aspace)
        .expect("fork-style process");
    let forked_leader = registry.lookup_task(102).expect("forked leader");
    assert_eq!(forked_leader.task_id, 102);
    assert_eq!(forked_leader.pid, forked);
    assert_eq!(registry.lookup_process(forked).unwrap().parent_pid, Some(pid));

    assert!(registry.mark_task_exited(101, 7));
    assert!(matches!(
        registry.lookup_process(pid).unwrap().state,
        ProcessState::Running
    ));
    assert!(registry.mark_task_exited(100, 9));
    assert!(matches!(
        registry.lookup_process(pid).unwrap().state,
        ProcessState::Exited(9)
    ));
}
