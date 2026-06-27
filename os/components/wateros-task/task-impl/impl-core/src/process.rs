//! 进程语义 registry。
//!
//! 这里不参与调度决策；调度器仍只处理 `TaskId`。在 WaterOS 当前模型里，
//! `ProcessRegistry` 只维护 process 对共享资源的归属，以及 process 下有哪些 task。

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use api_v0::{
    AddressSpaceRef, CloneFlags, CwdRef, FileTableRef, ProcessDescriptor, ProcessId, ProcessState,
    ProcessTaskDescriptor, ProcessTaskRole, ProcessTaskState, ResourceHandle, ResourceLimit,
    SetResourceLimitError, SignalHandlersRef, TaskClearTid, TaskExitCode, TaskGroupId, TaskId,
    ThreadId,
};

#[derive(Clone, Debug)]
struct ProcessTask {
    task_id : TaskId,
    tid : ThreadId,
    state : ProcessTaskState,
    tls : usize,
    clear_child_tid : Option<TaskClearTid>,
}

impl ProcessTask {
    fn descriptor(&self, pid : ProcessId, leader_task_id : TaskId) -> ProcessTaskDescriptor {
        ProcessTaskDescriptor { task_id : self.task_id,
                                tid : self.tid,
                                pid,
                                role : if self.task_id == leader_task_id {
                                    ProcessTaskRole::Leader
                                } else {
                                    ProcessTaskRole::Member
                                },
                                state : self.state,
                                tls : self.tls,
                                clear_child_tid : self.clear_child_tid }
    }
}

#[derive(Clone, Debug)]
pub struct ProcessControlBlock {
    pid : ProcessId,
    task_group_id : TaskGroupId,
    leader_task_id : TaskId,
    parent_pid : Option<ProcessId>,
    address_space : Option<AddressSpaceRef>,
    file_table : Option<FileTableRef>,
    cwd : Option<CwdRef>,
    signal_handlers : Option<SignalHandlersRef>,
    rlimits : BTreeMap<usize, ResourceLimit>,
    tasks : Vec<ProcessTask>,
    state : ProcessState,
    /// 已把 exit status 交给父进程 `waitpid`，但 leader 可能仍在 `sys_exit` 清理中。
    wait_status_delivered : bool,
}

impl ProcessControlBlock {
    fn descriptor(&self) -> ProcessDescriptor {
        ProcessDescriptor { pid : self.pid,
                            task_group_id : self.task_group_id,
                            leader_task_id : self.leader_task_id,
                            parent_pid : self.parent_pid,
                            address_space : self.address_space,
                            file_table : self.file_table,
                            cwd : self.cwd,
                            signal_handlers : self.signal_handlers,
                            task_count : self.tasks.len(),
                            state : self.state }
    }

    fn task_descriptor(&self, task_id : TaskId) -> Option<ProcessTaskDescriptor> {
        self.tasks
            .iter()
            .find(|task| task.task_id == task_id)
            .map(|task| task.descriptor(self.pid, self.leader_task_id))
    }
}

/// 单核 bring-up 用进程注册表。
pub struct ProcessRegistry {
    processes : BTreeMap<ProcessId, ProcessControlBlock>,
    task_index : BTreeMap<TaskId, ProcessId>,
    thread_index : BTreeMap<ThreadId, TaskId>,
    next_pid : usize,
    next_tid : usize,
}

impl ProcessRegistry {
    pub fn new() -> Self {
        Self { processes : BTreeMap::new(),
               task_index : BTreeMap::new(),
               thread_index : BTreeMap::new(),
               next_pid : 1,
               next_tid : 1 }
    }

    pub fn clear(&mut self) {
        self.processes
            .clear();
        self.task_index
            .clear();
        self.thread_index
            .clear();
        self.next_pid = 1;
        self.next_tid = 1;
    }

    fn alloc_pid(&mut self) -> ProcessId {
        loop {
            let pid = ProcessId::from_raw(self.next_pid);
            self.next_pid = self.next_pid
                                .saturating_add(1);
            if !self.processes
                    .contains_key(&pid) &&
               !self.thread_index
                    .contains_key(&ThreadId::from_raw(pid.raw()))
            {
                return pid;
            }
        }
    }

    fn alloc_tid(&mut self) -> ThreadId {
        loop {
            let tid = ThreadId::from_raw(self.next_tid);
            self.next_tid = self.next_tid
                                .saturating_add(1);
            if !self.thread_index
                    .contains_key(&tid)
            {
                return tid;
            }
        }
    }

    pub fn create_process_for_task(&mut self,
                                   task_id : TaskId,
                                   parent_pid : Option<ProcessId>,
                                   address_space : Option<AddressSpaceRef>)
                                   -> ProcessId {
        assert!(!self.task_index
                     .contains_key(&task_id),
                "task {} is already registered in a process",
                task_id);
        let pid = self.alloc_pid();
        if self.next_tid <= pid.raw() {
            self.next_tid = pid.raw()
                               .saturating_add(1);
        }
        let tid = ThreadId::from_raw(pid.raw());
        let resource_handle = ResourceHandle::from_raw(task_id);
        let process = ProcessControlBlock { pid,
                                            task_group_id : TaskGroupId::from_raw(pid.raw()),
                                            leader_task_id : task_id,
                                            parent_pid,
                                            address_space,
                                            file_table : Some(resource_handle),
                                            cwd : Some(resource_handle),
                                            signal_handlers : None,
                                            rlimits : BTreeMap::new(),
                                            tasks : alloc::vec![ProcessTask {
                task_id,
                tid,
                state: ProcessTaskState::Runnable,
                tls: 0,
                clear_child_tid: None,
            }],
                                            state : ProcessState::Running,
                                            wait_status_delivered : false };
        self.insert_process(process);
        pid
    }

    pub fn create_process_like_fork(&mut self,
                                    parent_pid : ProcessId,
                                    child_task_id : TaskId,
                                    address_space : Option<AddressSpaceRef>)
                                    -> Option<ProcessId> {
        let parent_rlimits = self.processes
                                 .get(&parent_pid)?
                                 .rlimits
                                 .clone();
        let child_pid = self.create_process_for_task(child_task_id,
                                                     Some(parent_pid),
                                                     address_space);
        if let Some(process) = self.process_mut(child_pid) {
            process.rlimits = parent_rlimits;
        }
        Some(child_pid)
    }

    pub fn get_process_rlimit(&self, pid : ProcessId, resource : usize) -> Option<ResourceLimit> {
        self.processes
            .get(&pid)
            .and_then(|process| {
                process.rlimits
                       .get(&resource)
                       .copied()
            })
    }

    pub fn set_process_rlimit(&mut self,
                              pid : ProcessId,
                              resource : usize,
                              limit : ResourceLimit)
                              -> Result<(), SetResourceLimitError> {
        if limit.cur > limit.max {
            return Err(SetResourceLimitError::InvalidArgument);
        }
        let process = self.process_mut(pid)
                          .ok_or(SetResourceLimitError::InvalidArgument)?;
        process.rlimits
               .insert(resource, limit);
        Ok(())
    }

    pub fn add_task_to_process(&mut self,
                               pid : ProcessId,
                               task_id : TaskId,
                               clone_flags : CloneFlags,
                               tls : usize,
                               clear_child_tid : Option<TaskClearTid>)
                               -> Option<TaskId> {
        if self.task_index.contains_key(&task_id) {
            return None;
        }
        let tid = self.alloc_tid();
        {
            let process = self.process_mut(pid)?;
            process.tasks
                   .push(ProcessTask { task_id,
                                       tid,
                                       state : ProcessTaskState::Runnable,
                                       tls : if clone_flags.contains(CloneFlags::CLONE_SETTLS) {
                                           tls
                                       } else {
                                           0
                                       },
                                       clear_child_tid });
        }
        self.task_index
            .insert(task_id, pid);
        self.thread_index
            .insert(tid, task_id);
        Some(task_id)
    }

    pub fn mark_task_exited(&mut self, task_id : TaskId, exit_code : TaskExitCode) -> bool {
        let Some(pid) = self.task_index
                            .get(&task_id)
                            .copied()
        else {
            return false;
        };
        let Some(process) = self.process_mut(pid) else {
            return false;
        };
        let Some(task) = process.tasks
                                .iter_mut()
                                .find(|task| task.task_id == task_id)
        else {
            return false;
        };
        task.state = ProcessTaskState::Exited(exit_code);
        if process.tasks
                  .iter()
                  .all(|task| matches!(task.state, ProcessTaskState::Exited(_)))
        {
            process.state = ProcessState::Exited(exit_code);
        }
        true
    }

    pub fn mark_process_exiting(&mut self, pid : ProcessId, exit_code : TaskExitCode) -> bool {
        // 父进程死亡（exit_group）时立即将子进程托孤给 init，
        // 匹配 Linux 语义：子进程 getppid() 应返回 1（init）。
        // 必须在借用 self.process_mut(pid) 之前完成托孤。
        self.reparent_orphans(pid);
        let Some(process) = self.process_mut(pid) else {
            return false;
        };
        // sys_exit 与 exit_group_current 都会调用本函数；若父进程已通过
        // wait_report_exited_child 交付过 status，不得撤销 wait_status_delivered，
        // 否则 zombie 会“复活”并再次阻塞 shell 的 waitpid。
        let preserve_wait_status = process.wait_status_delivered;
        process.state = ProcessState::Exiting(exit_code);
        for task in &mut process.tasks {
            task.state = ProcessTaskState::Exited(exit_code);
        }
        process.state = ProcessState::Exited(exit_code);
        if !preserve_wait_status {
            process.wait_status_delivered = false;
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        true
    }

    pub fn mark_wait_status_delivered(&mut self, pid : ProcessId) -> bool {
        let Some(process) = self.process_mut(pid) else {
            return false;
        };
        process.wait_status_delivered = true;
        true
    }

    pub fn process_wait_status_delivered(&self, pid : ProcessId) -> bool {
        self.processes
            .get(&pid)
            .is_some_and(|process| process.wait_status_delivered)
    }

    /// leader 线程结束清理后，回收已 wait 过、仍占槽的 zombie process。
    pub fn finalize_wait_delivered_process(&mut self, pid : ProcessId) -> bool {
        let should_reap = self.processes
                              .get(&pid)
                              .is_some_and(|process| {
                                  process.wait_status_delivered &&
                                  matches!(process.state, ProcessState::Exited(_))
                              });
        if !should_reap {
            return false;
        }
        self.reap_process_with_tasks(pid)
            .is_some()
    }

    pub fn exit_code_for_process(&self, pid : ProcessId) -> Option<TaskExitCode> {
        let process = self.processes
                          .get(&pid)?;
        match process.state {
            ProcessState::Exited(exit_code) | ProcessState::Exiting(exit_code) => Some(exit_code),
            ProcessState::Running => None,
        }
    }

    pub fn task_ids_for_process(&self, pid : ProcessId) -> Option<Vec<TaskId>> {
        let process = self.processes
                          .get(&pid)?;
        Some(process.tasks
                    .iter()
                    .map(|task| task.task_id)
                    .collect())
    }

    pub fn task_id_for_thread(&self, tid : ThreadId) -> Option<TaskId> {
        self.thread_index
            .get(&tid)
            .copied()
    }

    pub fn task_exit_would_finish_process(&self, task_id : TaskId) -> Option<bool> {
        let pid = self.task_index
                      .get(&task_id)
                      .copied()?;
        let process = self.processes
                          .get(&pid)?;
        Some(process.tasks
                    .iter()
                    .all(|task| {
                        task.task_id == task_id ||
                        matches!(task.state, ProcessTaskState::Exited(_))
                    }))
    }

    pub fn retain_only_task_in_process(&mut self,
                                       pid : ProcessId,
                                       keep_task_id : TaskId)
                                       -> Option<Vec<TaskId>> {
        let process = self.process_mut(pid)?;
        let mut removed = Vec::new();
        process.tasks
               .retain(|task| {
                   let keep = task.task_id == keep_task_id;
                   if !keep {
                       removed.push((task.task_id, task.tid));
                   }
                   keep
               });
        process.leader_task_id = keep_task_id;
        process.state = ProcessState::Running;
        for (task_id, tid) in &removed {
            self.task_index
                .remove(task_id);
            self.thread_index
                .remove(tid);
        }
        Some(removed.into_iter().map(|(task_id, _)| task_id).collect())
    }

    pub fn take_exited_member_tasks(&mut self, pid : ProcessId) -> Option<Vec<TaskId>> {
        let process = self.process_mut(pid)?;
        let leader_task_id = process.leader_task_id;
        let mut removed = Vec::new();
        process.tasks
               .retain(|task| {
                   let reap = task.task_id != leader_task_id &&
                              matches!(task.state, ProcessTaskState::Exited(_));
                   if reap {
                       removed.push((task.task_id, task.tid));
                   }
                   !reap
               });
        for (task_id, tid) in &removed {
            self.task_index
                .remove(task_id);
            self.thread_index
                .remove(tid);
        }
        Some(removed.into_iter().map(|(task_id, _)| task_id).collect())
    }

    /// fork 失败回滚：移除仅含单任务、仍在 Running 的子进程并释放其地址空间。
    pub fn abort_forked_process(&mut self, child_task_id : TaskId) -> Option<ProcessId> {
        let pid = self.task_index
                      .get(&child_task_id)
                      .copied()?;
        let process = self.processes
                          .get(&pid)?;
        if process.tasks.len() != 1 || process.tasks[0].task_id != child_task_id {
            return None;
        }
        if !matches!(process.state, ProcessState::Running) {
            return None;
        }
        let process = self.processes
                          .remove(&pid)?;
        self.drop_process_indexes(&process);
        if let Some(aspace) = process.address_space {
            let ptr = aspace.user_aspace_ptr();
            if ptr != 0 {
                mm_api::user_aspace_lifecycle::drop_user_aspace_on_task_exit(ptr);
            }
        }
        Some(pid)
    }

    /// clone 线程创建失败回滚：从进程线程列表移除该任务（不释放共享地址空间）。
    pub fn abort_cloned_thread(&mut self, child_task_id : TaskId) -> bool {
        let Some(pid) = self.task_index
                            .get(&child_task_id)
                            .copied()
        else {
            return false;
        };
        let Some(process) = self.process_mut(pid) else {
            return false;
        };
        let mut removed_tid = None;
        process.tasks
               .retain(|task| {
                   let remove = task.task_id == child_task_id;
                   if remove {
                       removed_tid = Some(task.tid);
                   }
                   !remove
               });
        if let Some(tid) = removed_tid {
            self.task_index
                .remove(&child_task_id);
            self.thread_index
                .remove(&tid);
            return true;
        }
        false
    }

    pub fn set_task_clear_child_tid(&mut self,
                                    task_id : TaskId,
                                    clear_child_tid : Option<TaskClearTid>)
                                    -> bool {
        let Some(pid) = self.task_index
                            .get(&task_id)
                            .copied()
        else {
            return false;
        };
        let Some(process) = self.process_mut(pid) else {
            return false;
        };
        let Some(task) = process.tasks
                                .iter_mut()
                                .find(|task| task.task_id == task_id)
        else {
            return false;
        };
        task.clear_child_tid = clear_child_tid;
        true
    }

    pub fn task_clear_child_tid(&self, task_id : TaskId) -> Option<TaskClearTid> {
        let pid = self.task_index
                      .get(&task_id)
                      .copied()?;
        self.processes
            .get(&pid)?
            .tasks
            .iter()
            .find(|task| task.task_id == task_id)
            .and_then(|task| task.clear_child_tid)
    }

    pub fn update_process_address_space(&mut self,
                                        pid : ProcessId,
                                        address_space : Option<AddressSpaceRef>)
                                        -> bool {
        let Some(process) = self.process_mut(pid) else {
            return false;
        };
        process.address_space = address_space;
        true
    }

    pub fn lookup_process(&self, pid : ProcessId) -> Option<ProcessDescriptor> {
        self.processes
            .get(&pid)
            .map(ProcessControlBlock::descriptor)
    }

    pub fn lookup_task(&self, task_id : TaskId) -> Option<ProcessTaskDescriptor> {
        let pid = self.task_index
                      .get(&task_id)
                      .copied()?;
        self.processes
            .get(&pid)?
            .task_descriptor(task_id)
    }

    pub fn leader_task_for_process(&self, pid : ProcessId) -> Option<TaskId> {
        self.processes
            .get(&pid)
            .map(|process| process.leader_task_id)
    }

    pub fn find_exited_child_process(&self, parent_pid : ProcessId) -> Option<ProcessDescriptor> {
        self.processes
            .values()
            .find(|process| {
                process.parent_pid == Some(parent_pid) &&
                !process.wait_status_delivered &&
                matches!(process.state, ProcessState::Exited(_))
            })
            .map(ProcessControlBlock::descriptor)
    }

    pub fn find_exited_child_process_by_pid(&self,
                                            parent_pid : ProcessId,
                                            child_pid : ProcessId)
                                            -> Option<ProcessDescriptor> {
        let process = self.processes
                          .get(&child_pid)?;
        if process.parent_pid != Some(parent_pid) ||
           process.wait_status_delivered ||
           !matches!(process.state, ProcessState::Exited(_))
        {
            return None;
        }
        Some(process.descriptor())
    }

    pub fn has_child_process(&self, parent_pid : ProcessId) -> bool {
        self.processes
            .values()
            .any(|process| {
                process.parent_pid == Some(parent_pid) && !process.wait_status_delivered
            })
    }

    /// 收集指定父进程的所有子进程 PID。
    pub fn collect_child_pids(&self, parent_pid : ProcessId) -> Vec<ProcessId> {
        self.processes
            .values()
            .filter(|process| process.parent_pid == Some(parent_pid))
            .map(|process| process.pid)
            .collect()
    }

    /// 将指定进程的所有子进程托孤给 init（PID 1）。
    ///
    /// 父进程退出时调用，确保子进程的 `parent_pid` 始终指向有效进程，
    /// 避免子进程 exit 后成为无人回收的僵尸。
    fn reparent_orphans(&mut self, parent_pid : ProcessId) {
        const INIT_PID : usize = 1;
        if parent_pid.raw() == INIT_PID {
            return;
        }
        let children = self.collect_child_pids(parent_pid);
        if children.is_empty() {
            return;
        }
        let init_pid = ProcessId::from_raw(INIT_PID);
        for child_pid in &children {
            if let Some(process) = self.process_mut(*child_pid) {
                process.parent_pid = Some(init_pid);
            }
        }
    }

    /// 列出 registry 中仍占槽的全部进程 id（含 Running / Exited）。
    pub fn all_process_pids(&self) -> Vec<ProcessId> {
        self.processes
            .values()
            .map(|process| process.pid)
            .collect()
    }

    /// 列出 registry 中所有已退出、尚未 reap 的进程。
    pub fn collect_exited_process_pids(&self) -> Vec<ProcessId> {
        self.processes
            .values()
            .filter(|process| matches!(process.state, ProcessState::Exited(_)))
            .map(|process| process.pid)
            .collect()
    }

    pub fn reap_process(&mut self, pid : ProcessId) -> Option<ProcessDescriptor> {
        self.reap_process_with_tasks(pid)
            .map(|(descriptor, _)| descriptor)
    }

    pub fn reap_process_with_tasks(&mut self,
                                   pid : ProcessId)
                                   -> Option<(ProcessDescriptor, Vec<TaskId>)> {
        let process = self.processes
                          .get(&pid)?;
        if !matches!(process.state, ProcessState::Exited(_)) {
            return None;
        }
        // 托孤：在移除本进程前，将所有子进程的 parent_pid 重定向到 init
        self.reparent_orphans(pid);
        self.processes
            .remove(&pid)
            .map(|process| {
                self.drop_process_indexes(&process);
                if let Some(aspace) = process.address_space {
                    let ptr = aspace.user_aspace_ptr();
                    if ptr != 0 {
                        mm_api::user_aspace_lifecycle::drop_user_aspace_on_task_exit(ptr);
                    }
                }
                let task_ids = process.tasks
                                      .iter()
                                      .map(|task| task.task_id)
                                      .collect();
                (process.descriptor(), task_ids)
            })
    }

    fn insert_process(&mut self, process : ProcessControlBlock) {
        let pid = process.pid;
        assert!(!self.processes
                     .contains_key(&pid),
                "process slot {} already occupied",
                pid.raw());
        for task in &process.tasks {
            assert!(!self.task_index
                         .contains_key(&task.task_id),
                    "task {} is already indexed",
                    task.task_id);
            assert!(!self.thread_index
                         .contains_key(&task.tid),
                    "thread {} is already indexed",
                    task.tid.raw());
        }
        for task in &process.tasks {
            self.task_index
                .insert(task.task_id, pid);
            self.thread_index
                .insert(task.tid, task.task_id);
        }
        self.processes
            .insert(pid, process);
    }

    fn process_mut(&mut self, pid : ProcessId) -> Option<&mut ProcessControlBlock> {
        self.processes
            .get_mut(&pid)
    }

    fn drop_process_indexes(&mut self, process : &ProcessControlBlock) {
        for task in &process.tasks {
            self.task_index
                .remove(&task.task_id);
            self.thread_index
                .remove(&task.tid);
        }
    }
}
