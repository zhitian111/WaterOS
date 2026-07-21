//! 进程语义 registry。
//!
//! 这里不参与调度决策；调度器仍只处理 `TaskId`。在 WaterOS 当前模型里，
//! `ProcessRegistry` 只维护 process 对共享资源的归属，以及 process 下有哪些 task。

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use api_v0::{
    AddressSpaceRef, CloneFlags, ProcessDescriptor, ProcessId, ProcessState, ProcessTaskDescriptor,
    ProcessTaskRole, ProcessTaskState, ResourceLimit, SetResourceLimitError, TaskClearTid,
    TaskExitCode, TaskId, ThreadId,
};

#[derive(Clone, Debug)]
struct ProcessTask {
    task_id : TaskId,
    tid : ThreadId,
    state : ProcessTaskState,
    //线程本地存储（Thread-Local Storage）基址。
    tls : usize,
    //TaskClearTid 是一个用户态地址（指针），内核在该线程最后一次退出时需要往该地址写 0 并唤醒 futex 等待者。
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
    leader_task_id : TaskId,
    parent_pid : Option<ProcessId>,
    address_space : Option<AddressSpaceRef>,
    rlimits : BTreeMap<usize, ResourceLimit>,
    /// LTP `setpriority`/`getpriority` 变量；不参与调度。
    nice : i32,
    /// 进程组 id；fork 继承，由 `setpgid` 更新。
    pgid : ProcessId,
    tasks : Vec<ProcessTask>,
    state : ProcessState,
    //会话 ID（Session ID）。由 setsid() 系统调用设置，表示进程所属的会话（session）。会话是一组进程组的集合，通常与终端登录相关。
    sid : ProcessId,
    //是否允许 core dump。Linux 中控制进程在崩溃时能否产生 core dump 文件
    dumpable : bool,
    //子进程代管标记。Linux PR_SET_CHILD_SUBREAPER 机制
    child_subreaper : bool,
    //SIGSTOP/SIGTSTP/SIGTTIN/SIGTTOU 等待通知标记。
    stop_wait_pending : bool,
    //SIGCONT 等待通知标记。
    continued_wait_pending : bool,
}

impl ProcessControlBlock {
    fn descriptor(&self) -> ProcessDescriptor {
        ProcessDescriptor { pid : self.pid,
                            leader_task_id : self.leader_task_id,
                            parent_pid : self.parent_pid,
                            address_space : self.address_space,
                            task_count : self.tasks.len(),
                            state : self.state,
                            pgid : self.pgid,
                            sid : self.sid }
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
    //进程号有序，范围查询潜力，确定性迭代，稳定的 hash 性能
    processes : BTreeMap<ProcessId, ProcessControlBlock>,
    next_pid : usize,
    next_tid : usize,
}

impl ProcessRegistry {
    pub fn new() -> Self {
        Self { processes : BTreeMap::new(),
               next_pid : 1,
               next_tid : 1 }
    }

    pub fn clear(&mut self) {
        self.processes
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
               self.task_id_for_thread(ThreadId::from_raw(pid.raw()))
                   .is_none()
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
            if self.task_id_for_thread(tid)
                   .is_none()
            {
                return tid;
            }
        }
    }
    // 根据taskid创建进程，返回pid
    pub fn create_process_for_task(&mut self,
                                   task_id : TaskId,
                                   parent_pid : Option<ProcessId>,
                                   address_space : Option<AddressSpaceRef>)
                                   -> ProcessId {
        // 确保 task_id 尚未注册到任何进程。
        assert!(self.lookup_task(task_id)
                    .is_none(),
                "task {} is already registered in a process",
                task_id);
        let pid = self.alloc_pid();
        //保证 next_tid 始终大于或等于已分配的 PID 值，避免 TID 与已分配的 PID 冲突。
        if self.next_tid <= pid.raw() {
            self.next_tid = pid.raw()
                               .saturating_add(1);
        }
        let tid = ThreadId::from_raw(pid.raw());
        let process = ProcessControlBlock { pid,
                                            leader_task_id : task_id,
                                            parent_pid,
                                            address_space,
                                            rlimits : BTreeMap::new(),
                                            nice : 0,
                                            pgid : pid,
                                            tasks : alloc::vec![ProcessTask {
                task_id,
                tid,
                state: ProcessTaskState::Runnable,
                tls: 0,
                clear_child_tid: None,
            }],
                                            state : ProcessState::Running,
                                            sid : ProcessId::from_raw(0),
                                            dumpable : true,
                                            child_subreaper : false,
                                            stop_wait_pending : false,
                                            continued_wait_pending : false };
        self.insert_process(process);
        pid
    }
    // 根据父进程pid创建子进程，返回子进程pid
    pub fn create_process_like_fork(&mut self,
                                    parent_pid : ProcessId,
                                    child_task_id : TaskId,
                                    address_space : Option<AddressSpaceRef>)
                                    -> Option<ProcessId> {
        let parent = self.processes
                         .get(&parent_pid)?;
        let parent_rlimits = parent.rlimits
                                   .clone();
        let parent_nice = parent.nice;
        let parent_pgid = parent.pgid;
        let parent_sid = parent.sid;
        let parent_dumpable = parent.dumpable;
        let parent_subreaper = parent.child_subreaper;
        let child_pid = self.create_process_for_task(child_task_id,
                                                     Some(parent_pid),
                                                     address_space);
        if let Some(process) = self.process_mut(child_pid) {
            process.rlimits = parent_rlimits;
            process.nice = parent_nice;
            process.pgid = parent_pgid;
            process.sid = parent_sid;
            process.dumpable = parent_dumpable;
            process.child_subreaper = parent_subreaper;
        }
        Some(child_pid)
    }

    pub fn get_process_nice(&self, pid : ProcessId) -> Option<i32> {
        self.processes
            .get(&pid)
            .map(|process| process.nice)
    }

    pub fn set_process_nice(&mut self, pid : ProcessId, nice : i32) -> bool {
        let Some(process) = self.process_mut(pid) else {
            return false;
        };
        process.nice = nice;
        true
    }

    pub fn get_process_pgid(&self, pid : ProcessId) -> Option<ProcessId> {
        self.processes
            .get(&pid)
            .map(|process| process.pgid)
    }

    pub fn set_process_pgid(&mut self, pid : ProcessId, pgid : ProcessId) -> bool {
        let Some(process) = self.process_mut(pid) else {
            return false;
        };
        process.pgid = pgid;
        true
    }

    pub fn set_nice_for_pgid(&mut self, pgid : ProcessId, nice : i32) -> bool {
        let mut found = false;
        for process in self.processes
                           .values_mut()
        {
            if process.pgid == pgid {
                process.nice = nice;
                found = true;
            }
        }
        found
    }

    /// `getpriority(PRIO_PGRP)`：组内最高优先级 = nice 最小值。
    pub fn min_nice_in_pgid(&self, pgid : ProcessId) -> Option<i32> {
        self.processes
            .values()
            .filter(|process| process.pgid == pgid)
            .map(|process| process.nice)
            .min()
    }

    pub fn process_exists(&self, pid : ProcessId) -> bool {
        self.processes
            .contains_key(&pid)
    }

    pub fn pgid_has_members(&self, pgid : ProcessId) -> bool {
        self.processes
            .values()
            .any(|process| process.pgid == pgid)
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
        if self.lookup_task(task_id)
               .is_some()
        {
            return None;
        }
        let tid = self.alloc_tid();
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
        Some(task_id)
    }

    pub fn mark_task_exited(&mut self, task_id : TaskId, exit_code : TaskExitCode) -> bool {
        for process in self.processes
                           .values_mut()
        {
            let Some(task) = process.tasks
                                    .iter_mut()
                                    .find(|task| task.task_id == task_id)
            else {
                continue;
            };
            task.state = ProcessTaskState::Exited(exit_code);
            if process.tasks
                      .iter()
                      .all(|task| matches!(task.state, ProcessTaskState::Exited(_)))
            {
                process.state = ProcessState::Exited(exit_code);
            }
            return true;
        }
        false
    }

    pub fn mark_process_exiting(&mut self, pid : ProcessId, exit_code : TaskExitCode) -> bool {
        // 父进程死亡（exit_group）时立即将子进程托孤给 init，
        // 匹配 Linux 语义：子进程 getppid() 应返回 1（init）。
        // 必须在借用 self.process_mut(pid) 之前完成托孤。
        self.reparent_orphans(pid);
        let Some(process) = self.process_mut(pid) else {
            return false;
        };
        process.state = ProcessState::Exiting(exit_code);
        for task in &mut process.tasks {
            task.state = ProcessTaskState::Exited(exit_code);
        }
        process.state = ProcessState::Exited(exit_code);
        true
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
        self.processes
            .values()
            .find_map(|process| {
                process.tasks
                       .iter()
                       .find(|task| task.tid == tid)
                       .map(|task| task.task_id)
            })
    }

    pub fn task_exit_would_finish_process(&self, task_id : TaskId) -> Option<bool> {
        let process = self.processes
                          .values()
                          .find(|process| {
                              process.tasks
                                     .iter()
                                     .any(|task| task.task_id == task_id)
                          })?;
        Some(process.tasks
                    .iter()
                    .all(|task| {
                        task.task_id == task_id || matches!(task.state, ProcessTaskState::Exited(_))
                    }))
    }
    // 移除进程中除了指定 task_id 外的所有任务，并将该 task_id 设为 leader。
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
                       removed.push(task.task_id);
                   }
                   keep
               });
        process.leader_task_id = keep_task_id;
        process.state = ProcessState::Running;
        Some(removed)
    }
    // 移除进程中所有已退出的 member 任务，并返回被移除的 task_id 列表。
    pub fn take_exited_member_tasks(&mut self, pid : ProcessId) -> Option<Vec<TaskId>> {
        let process = self.process_mut(pid)?;
        let leader_task_id = process.leader_task_id;
        let mut removed = Vec::new();
        process.tasks
               .retain(|task| {
                   let reap = task.task_id != leader_task_id &&
                              matches!(task.state, ProcessTaskState::Exited(_));
                   if reap {
                       removed.push(task.task_id);
                   }
                   !reap
               });
        Some(removed)
    }

    /// fork 失败回滚：移除仅含单任务、仍在 Running 的子进程并释放其地址空间。
    pub fn abort_forked_process(&mut self, child_task_id : TaskId) -> Option<ProcessId> {
        let pid = self.lookup_task(child_task_id)?
                      .pid;
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
        for process in self.processes
                           .values_mut()
        {
            let before = process.tasks.len();
            process.tasks
                   .retain(|task| task.task_id != child_task_id);
            if process.tasks.len() < before {
                return true;
            }
        }
        false
    }
    // 设置指定 task 的 clear_child_tid 字段，返回是否成功
    pub fn set_task_clear_child_tid(&mut self,
                                    task_id : TaskId,
                                    clear_child_tid : Option<TaskClearTid>)
                                    -> bool {
        for process in self.processes
                           .values_mut()
        {
            let Some(task) = process.tasks
                                    .iter_mut()
                                    .find(|task| task.task_id == task_id)
            else {
                continue;
            };
            task.clear_child_tid = clear_child_tid;
            return true;
        }
        false
    }
    // 获取指定 task 的 clear_child_tid 字段，返回 Option<TaskClearTid>
    pub fn task_clear_child_tid(&self, task_id : TaskId) -> Option<TaskClearTid> {
        self.processes
            .values()
            .find_map(|process| {
                process.tasks
                       .iter()
                       .find(|task| task.task_id == task_id)
                       .and_then(|task| task.clear_child_tid)
            })
    }
    // 获取指定进程的地址空间，返回 Option<AddressSpaceRef>
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
        self.processes
            .values()
            .find_map(|process| process.task_descriptor(task_id))
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
                matches!(process.state, ProcessState::Exited(_))
            })
            .map(ProcessControlBlock::descriptor)
    }

    pub fn find_exited_child_process_in_pgid(&self,
                                             parent_pid : ProcessId,
                                             pgid : ProcessId)
                                             -> Option<ProcessDescriptor> {
        self.processes
            .values()
            .find(|process| {
                process.parent_pid == Some(parent_pid) &&
                process.pgid == pgid &&
                matches!(process.state, ProcessState::Exited(_))
            })
            .map(ProcessControlBlock::descriptor)
    }

    pub fn has_child_process(&self, parent_pid : ProcessId) -> bool {
        self.processes
            .values()
            .any(|process| process.parent_pid == Some(parent_pid))
    }

    pub fn has_child_process_in_pgid(&self, parent_pid : ProcessId, pgid : ProcessId) -> bool {
        self.processes
            .values()
            .any(|process| process.parent_pid == Some(parent_pid) && process.pgid == pgid)
    }

    pub fn mark_process_stopped(&mut self, pid : ProcessId, signo : u8) -> bool {
        let process = match self.process_mut(pid) {
            Some(process) => process,
            None => return false,
        };
        if matches!(process.state,
                    ProcessState::Exited(_) | ProcessState::Exiting(_))
        {
            return false;
        }
        if matches!(process.state,
                    ProcessState::Stopped { .. })
        {
            return true;
        }
        process.state = ProcessState::Stopped { signo };
        process.stop_wait_pending = true;
        process.continued_wait_pending = false;
        true
    }

    pub fn mark_process_continued(&mut self, pid : ProcessId) -> bool {
        let process = match self.process_mut(pid) {
            Some(process) => process,
            None => return false,
        };
        if !matches!(process.state,
                     ProcessState::Stopped { .. })
        {
            return false;
        }
        process.state = ProcessState::Running;
        process.continued_wait_pending = true;
        process.stop_wait_pending = false;
        true
    }

    pub fn consume_stop_wait(&mut self, pid : ProcessId, nowait : bool) {
        let Some(process) = self.process_mut(pid) else {
            return;
        };
        if !nowait {
            process.stop_wait_pending = false;
        }
    }

    pub fn consume_continued_wait(&mut self, pid : ProcessId, nowait : bool) {
        let Some(process) = self.process_mut(pid) else {
            return;
        };
        if !nowait {
            process.continued_wait_pending = false;
        }
    }

    pub fn find_stopped_child_process(&self, parent_pid : ProcessId) -> Option<ProcessDescriptor> {
        self.processes
            .values()
            .find(|process| {
                process.parent_pid == Some(parent_pid) &&
                process.stop_wait_pending &&
                matches!(process.state,
                         ProcessState::Stopped { .. })
            })
            .map(ProcessControlBlock::descriptor)
    }

    pub fn stopped_child_ready_for_wait(&self,
                                        parent_pid : ProcessId,
                                        child_pid : ProcessId)
                                        -> Option<ProcessDescriptor> {
        let process = self.processes
                          .get(&child_pid)?;
        if process.parent_pid != Some(parent_pid) || !process.stop_wait_pending {
            return None;
        }
        if !matches!(process.state,
                     ProcessState::Stopped { .. })
        {
            return None;
        }
        Some(process.descriptor())
    }

    pub fn find_stopped_child_process_in_pgid(&self,
                                              parent_pid : ProcessId,
                                              pgid : ProcessId)
                                              -> Option<ProcessDescriptor> {
        self.processes
            .values()
            .find(|process| {
                process.parent_pid == Some(parent_pid) &&
                process.pgid == pgid &&
                process.stop_wait_pending &&
                matches!(process.state,
                         ProcessState::Stopped { .. })
            })
            .map(ProcessControlBlock::descriptor)
    }

    pub fn find_continued_child_process(&self,
                                        parent_pid : ProcessId)
                                        -> Option<ProcessDescriptor> {
        self.processes
            .values()
            .find(|process| {
                process.parent_pid == Some(parent_pid) &&
                process.continued_wait_pending &&
                matches!(process.state, ProcessState::Running)
            })
            .map(ProcessControlBlock::descriptor)
    }

    pub fn continued_child_ready_for_wait(&self,
                                          parent_pid : ProcessId,
                                          child_pid : ProcessId)
                                          -> Option<ProcessDescriptor> {
        let process = self.processes
                          .get(&child_pid)?;
        if process.parent_pid != Some(parent_pid) || !process.continued_wait_pending {
            return None;
        }
        if !matches!(process.state, ProcessState::Running) {
            return None;
        }
        Some(process.descriptor())
    }

    pub fn find_continued_child_process_in_pgid(&self,
                                                parent_pid : ProcessId,
                                                pgid : ProcessId)
                                                -> Option<ProcessDescriptor> {
        self.processes
            .values()
            .find(|process| {
                process.parent_pid == Some(parent_pid) &&
                process.pgid == pgid &&
                process.continued_wait_pending &&
                matches!(process.state, ProcessState::Running)
            })
            .map(ProcessControlBlock::descriptor)
    }

    pub fn create_session_for_process(&mut self, pid : ProcessId) -> Result<(), ()> {
        let process = self.process_mut(pid)
                          .ok_or(())?;
        if process.pgid == pid {
            return Err(());
        }
        process.sid = pid;
        process.pgid = pid;
        Ok(())
    }

    pub fn process_dumpable(&self, pid : ProcessId) -> Option<bool> {
        self.processes
            .get(&pid)
            .map(|process| process.dumpable)
    }

    pub fn set_process_dumpable(&mut self, pid : ProcessId, dumpable : bool) -> bool {
        let Some(process) = self.process_mut(pid) else {
            return false;
        };
        process.dumpable = dumpable;
        true
    }

    pub fn process_child_subreaper(&self, pid : ProcessId) -> Option<bool> {
        self.processes
            .get(&pid)
            .map(|process| process.child_subreaper)
    }

    pub fn set_process_child_subreaper(&mut self, pid : ProcessId, enabled : bool) -> bool {
        let Some(process) = self.process_mut(pid) else {
            return false;
        };
        process.child_subreaper = enabled;
        true
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
        self.processes
            .insert(pid, process);
    }

    fn process_mut(&mut self, pid : ProcessId) -> Option<&mut ProcessControlBlock> {
        self.processes
            .get_mut(&pid)
    }
}
