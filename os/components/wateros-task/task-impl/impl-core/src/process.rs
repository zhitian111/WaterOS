//! 进程语义 registry。
//!
//! 这里不参与调度决策；调度器仍只处理 `TaskId`。在 WaterOS 当前模型里，
//! `ProcessRegistry` 只维护 process 对共享资源的归属，以及 process 下有哪些 task。

use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;

use api_v0::{
    AddressSpaceRef, CloneFlags, ProcessError, ProcessId, ProcessResult, ProcessSnapshot,
    ProcessState, ProcessTaskRole, ProcessTaskSnapshot, ProcessTaskState, ResourceLimit,
    TaskClearTid, TaskExitCode, TaskId, ThreadId,
};

#[derive(Clone, Debug)]
struct ProcessTask {
    tid : ThreadId,
    state : ProcessTaskState,
    //线程本地存储（Thread-Local Storage）基址。
    tls : usize,
    //TaskClearTid 是一个用户态地址（指针），内核在该线程最后一次退出时需要往该地址写 0 并唤醒 futex 等待者。
    clear_child_tid : Option<TaskClearTid>,
    /// 线程名（`prctl PR_SET_NAME`），上限 16 字节（含 NUL）。
    comm : [u8; 16],
}

impl ProcessTask {
    fn snapshot(&self,
                task_id : TaskId,
                pid : ProcessId,
                leader_task_id : TaskId)
                -> ProcessTaskSnapshot {
        ProcessTaskSnapshot { task_id,
                              tid : self.tid,
                              pid,
                              role : if task_id == leader_task_id {
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
    /// 进程组 id；fork 继承，由 `setpgid` 更新。
    pgid : ProcessId,
    tasks : BTreeMap<TaskId, ProcessTask>,

    /// 已退出的非 leader 线程。退出路径只向这里登记；维护路径按此队列精确
    /// 回收，避免每次 `exit` 都扫描整个 `tasks` 映射。
    exited_member_task_ids : VecDeque<TaskId>,
    /// exec 清理旧线程组期间禁止注册新的 member。
    exec_in_progress : bool,
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
    /// 文件创建权限掩码（umask）；fork 时继承。
    umask : u32,
    /// 父进程死亡时发送给本进程的信号（`prctl PR_SET_PDEATHSIG`）；0 表示未设置。
    parent_death_signal : i32,
}

/// A process removed from the registry whose owned resources can be dropped
/// after the registry lock has been released.
pub(crate) struct RetiredProcess {
    process : ProcessControlBlock,
}

impl RetiredProcess {
    pub(crate) fn cleanup(self) -> (ProcessSnapshot, Vec<TaskId>) {
        let snapshot = self.process.snapshot();
        let task_ids = self.process
                           .tasks
                           .keys()
                           .copied()
                           .collect();
        if let Some(aspace) = self.process.address_space {
            let ptr = aspace.user_aspace_ptr();
            if ptr != 0 {
                mm_api::user_aspace_lifecycle::drop_user_aspace_on_task_exit(ptr);
            }
        }
        (snapshot, task_ids)
    }
}

impl ProcessControlBlock {
    fn snapshot(&self) -> ProcessSnapshot {
        ProcessSnapshot { pid : self.pid,
                          leader_task_id : self.leader_task_id,
                          parent_pid : self.parent_pid,
                          address_space : self.address_space,
                          task_count : self.tasks.len(),
                          state : self.state,
                          pgid : self.pgid,
                          sid : self.sid }
    }

    fn ptask_snapshot(&self, task_id : TaskId) -> Option<ProcessTaskSnapshot> {
        self.tasks
            .get(&task_id)
            .map(|task| task.snapshot(task_id, self.pid, self.leader_task_id))
    }
}

/// 单核 bring-up 用进程注册表。
pub struct ProcessRegistry {
    //进程号有序，范围查询潜力，确定性迭代，稳定的 hash 性能
    processes : BTreeMap<ProcessId, ProcessControlBlock>,
    pid_for_task : BTreeMap<TaskId, ProcessId>,
    task_for_thread : BTreeMap<ThreadId, TaskId>,
    next_pid : usize,
    next_tid : usize,
}

impl ProcessRegistry {
    pub fn new() -> Self {
        Self { processes : BTreeMap::new(),
               pid_for_task : BTreeMap::new(),
               task_for_thread : BTreeMap::new(),
               next_pid : 1,
               next_tid : 1 }
    }

    pub fn clear(&mut self) {
        self.pid_for_task
            .clear();
        self.processes
            .clear();
        self.task_for_thread
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
               !self.task_for_thread
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
            if !self.task_for_thread
                    .contains_key(&tid)
            {
                return tid;
            }
        }
    }

    fn process_mut(&mut self, pid : ProcessId) -> Option<&mut ProcessControlBlock> {
        self.processes
            .get_mut(&pid)
    }

    /// 从两个反查索引中移除一个已从进程任务表删除的任务。
    fn remove_task_indexes(&mut self, pid : ProcessId, task_id : TaskId, tid : ThreadId) {
        assert_eq!(self.pid_for_task
                       .remove(&task_id),
                   Some(pid));
        assert_eq!(self.task_for_thread
                       .remove(&tid),
                   Some(task_id));
    }

    /// 移除完整进程并同步清理 task/tid 反查索引。
    fn remove_process(&mut self, pid : ProcessId) -> Option<ProcessControlBlock> {
        let process = self.processes
                          .remove(&pid)?;
        for (&task_id, task) in &process.tasks {
            assert_eq!(self.pid_for_task
                           .remove(&task_id),
                       Some(pid));
            assert_eq!(self.task_for_thread
                           .remove(&task.tid),
                       Some(task_id));
        }
        Some(process)
    }

    // 根据taskid创建进程，返回pid
    pub fn create_process_for_task(&mut self,
                                   task_id : TaskId,
                                   parent_pid : Option<ProcessId>,
                                   address_space : Option<AddressSpaceRef>)
                                   -> ProcessResult<ProcessId> {
        // 确保 task_id 尚未注册到任何进程。
        if self.pid_for_task
               .contains_key(&task_id)
        {
            return Err(ProcessError::InvalidArgument);
        }
        let pid = self.alloc_pid();
        //保证 next_tid 始终大于或等于已分配的 PID 值，避免 TID 与已分配的 PID 冲突。
        if self.next_tid <= pid.raw() {
            self.next_tid = pid.raw()
                               .saturating_add(1);
        }
        let tid = ThreadId::from_raw(pid.raw());
        let process_task = ProcessTask { tid,
                                         state : ProcessTaskState::Runnable,
                                         tls : 0,
                                         clear_child_tid : None,
                                         comm : [0u8; 16] };
        let mut map = BTreeMap::new();
        map.insert(task_id, process_task);
        let process = ProcessControlBlock { pid,
                                            leader_task_id : task_id,
                                            parent_pid,
                                            address_space,
                                            rlimits : BTreeMap::new(),
                                            pgid : pid,
                                            tasks : map,
                                            exited_member_task_ids : VecDeque::new(),
                                            exec_in_progress : false,
                                            state : ProcessState::Running,
                                            parent_death_signal : 0,
                                            sid : ProcessId::from_raw(0),
                                            dumpable : true,
                                            child_subreaper : false,
                                            stop_wait_pending : false,
                                            continued_wait_pending : false,
                                            umask : 0o022 };
        self.insert_process(process);
        assert_eq!(self.pid_for_task
                       .insert(task_id, pid),
                   None);
        assert_eq!(self.task_for_thread
                       .insert(tid, task_id),
                   None);
        Ok(pid)
    }
    // 根据父进程pid创建子进程，返回子进程pid
    pub fn create_process_like_fork(&mut self,
                                    parent_pid : ProcessId,
                                    child_task_id : TaskId,
                                    address_space : Option<AddressSpaceRef>)
                                    -> ProcessResult<ProcessId> {
        let parent = self.processes
                         .get(&parent_pid)
                         .ok_or(ProcessError::ProcessNotFound)?;
        let parent_rlimits = parent.rlimits
                                   .clone();
        let parent_pgid = parent.pgid;
        let parent_sid = parent.sid;
        let parent_dumpable = parent.dumpable;
        let parent_subreaper = parent.child_subreaper;
        let parent_umask = parent.umask;
        let child_pid = self.create_process_for_task(child_task_id,
                                                     Some(parent_pid),
                                                     address_space)?;
        if let Some(process) = self.process_mut(child_pid) {
            process.rlimits = parent_rlimits;
            process.pgid = parent_pgid;
            process.sid = parent_sid;
            process.dumpable = parent_dumpable;
            process.child_subreaper = parent_subreaper;
            process.umask = parent_umask;
        }
        Ok(child_pid)
    }

    pub fn get_process_umask(&self, pid : ProcessId) -> Option<u32> {
        self.processes
            .get(&pid)
            .map(|process| process.umask)
    }

    pub fn set_process_umask(&mut self, pid : ProcessId, mask : u32) -> ProcessResult<()> {
        let process = self.process_mut(pid)
                          .ok_or(ProcessError::ProcessNotFound)?;
        process.umask = mask;
        Ok(())
    }

    pub fn get_parent_death_signal(&self, pid : ProcessId) -> Option<i32> {
        self.processes
            .get(&pid)
            .map(|process| process.parent_death_signal)
    }

    pub fn set_parent_death_signal(&mut self, pid : ProcessId, sig : i32) -> ProcessResult<()> {
        let process = self.process_mut(pid)
                          .ok_or(ProcessError::ProcessNotFound)?;
        process.parent_death_signal = sig;
        Ok(())
    }

    pub fn get_thread_comm(&self, task_id : TaskId) -> Option<[u8; 16]> {
        let pid = self.pid_for_task
                      .get(&task_id)?;
        let process = self.processes
                          .get(pid)?;
        let task = process.tasks
                          .get(&task_id)?;
        Some(task.comm)
    }

    pub fn set_thread_comm(&mut self, task_id : TaskId, comm : [u8; 16]) -> ProcessResult<()> {
        let pid = self.pid_for_task
                      .get(&task_id)
                      .copied()
                      .ok_or(ProcessError::TaskNotFound)?;
        let task = self.processes
                       .get_mut(&pid)
                       .and_then(|process| {
                           process.tasks
                                  .get_mut(&task_id)
                       })
                       .ok_or(ProcessError::TaskNotFound)?;
        task.comm = comm;
        Ok(())
    }


    pub fn get_process_pgid(&self, pid : ProcessId) -> Option<ProcessId> {
        self.processes
            .get(&pid)
            .map(|process| process.pgid)
    }

    pub fn set_process_pgid(&mut self, pid : ProcessId, pgid : ProcessId) -> ProcessResult<()> {
        let process = self.process_mut(pid)
                          .ok_or(ProcessError::ProcessNotFound)?;
        process.pgid = pgid;
        Ok(())
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

    pub fn process_pids_in_pgid(&self, pgid : ProcessId) -> Vec<ProcessId> {
        self.processes
            .values()
            .filter(|process| process.pgid == pgid)
            .map(|process| process.pid)
            .collect()
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
                              -> ProcessResult<()> {
        if limit.cur > limit.max {
            return Err(ProcessError::InvalidArgument);
        }
        let process = self.process_mut(pid)
                          .ok_or(ProcessError::ProcessNotFound)?;
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
                               -> ProcessResult<TaskId> {
        if self.pid_for_task
               .contains_key(&task_id)
        {
            return Err(ProcessError::InvalidArgument);
        }
        let tid = self.alloc_tid();
        let process = self.process_mut(pid)
                          .ok_or(ProcessError::ProcessNotFound)?;
        if process.exec_in_progress {
            return Err(ProcessError::InvalidArgument);
        }
        process.tasks
               .insert(task_id, ProcessTask { tid,
                                              state:
                                                  ProcessTaskState::Runnable,
                                              tls:
                                                  if clone_flags.contains(CloneFlags::CLONE_SETTLS)
                                                  {
                                                      tls
                                                  } else {
                                                      0
                                                  },
                                              clear_child_tid,
                                              comm : [0u8;
                                                      16] });
        assert_eq!(self.pid_for_task
                       .insert(task_id, pid),
                   None);
        assert_eq!(self.task_for_thread
                       .insert(tid, task_id),
                   None);
        Ok(task_id)
    }

    /// 原子地阻止并发 clone，并返回 exec 需要清理的稳定线程列表。
    pub fn begin_process_exec(&mut self,
                              pid : ProcessId,
                              exec_task_id : TaskId)
                              -> ProcessResult<Vec<TaskId>> {
        let process = self.process_mut(pid)
                          .ok_or(ProcessError::ProcessNotFound)?;
        if process.exec_in_progress || process.leader_task_id != exec_task_id {
            return Err(ProcessError::InvalidArgument);
        }
        process.exec_in_progress = true;
        Ok(process.tasks.keys().copied().collect())
    }

    pub fn mark_task_exited(&mut self,
                            task_id : TaskId,
                            exit_code : TaskExitCode)
                            -> ProcessResult<bool> {
        let pid = self.pid_for_task
                      .get(&task_id)
                      .copied()
                      .ok_or(ProcessError::TaskNotFound)?;
        let process = self.processes
                          .get_mut(&pid)
                          .ok_or(ProcessError::ProcessNotFound)?;
        let process_was_exited = matches!(process.state, ProcessState::Exited(_));
        let task = process.tasks
                          .get_mut(&task_id)
                          .ok_or(ProcessError::TaskNotFound)?;
        let was_exited = matches!(task.state, ProcessTaskState::Exited(_));
        task.state = ProcessTaskState::Exited(exit_code);
        if task_id != process.leader_task_id && !was_exited {
            process.exited_member_task_ids
                   .push_back(task_id);
        }
        if process.tasks
                  .values()
                  .all(|task| matches!(task.state, ProcessTaskState::Exited(_)))
        {
            process.state = ProcessState::Exited(exit_code);
        }
        Ok(!process_was_exited && matches!(process.state, ProcessState::Exited(_)))
    }

    pub fn mark_process_exited(&mut self,
                               pid : ProcessId,
                               exit_code : TaskExitCode)
                               -> ProcessResult<()> {
        // 父进程死亡（exit_group）时立即将子进程托孤给 init，
        // 匹配 Linux 语义：子进程 getppid() 应返回 1（init）。
        // 必须在借用 self.process_mut(pid) 之前完成托孤。
        self.reparent_orphans(pid);
        let process = self.process_mut(pid)
                          .ok_or(ProcessError::ProcessNotFound)?;
        process.state = ProcessState::Exiting(exit_code);
        // Running siblings cannot be declared exited here: a parent could reap
        // the process and destroy its address space while they are still
        // executing on remote CPUs. Each sibling observes Exiting at its next
        // trap boundary and marks itself exited through mark_task_exited.
        if process.tasks
                  .values()
                  .all(|task| matches!(task.state, ProcessTaskState::Exited(_)))
        {
            process.state = ProcessState::Exited(exit_code);
        }
        Ok(())
    }

    pub fn task_ids_for_process(&self, pid : ProcessId) -> Option<Vec<TaskId>> {
        let process = self.processes
                          .get(&pid)?;
        Some(process.tasks
                    .keys()
                    .copied()
                    .collect())
    }

    /// 移除进程中除了指定 task_id 外的所有任务，并将该 task_id 设为 leader。
    pub fn retain_only_task_in_process(&mut self,
                                       pid : ProcessId,
                                       keep_task_id : TaskId)
                                       -> Option<Vec<TaskId>> {
        let removed = {
            let process = self.process_mut(pid)?;
            if !process.tasks
                       .contains_key(&keep_task_id)
            {
                return None;
            }
            let removed = process.tasks
                                 .keys()
                                 .copied()
                                 .filter(|task_id| *task_id != keep_task_id)
                                 .collect::<Vec<_>>();
            process.tasks
                   .retain(|task_id, _| *task_id == keep_task_id);
            process.leader_task_id = keep_task_id;
            process.exec_in_progress = false;
            process.state = ProcessState::Running;
            process.exited_member_task_ids
                   .clear();
            removed
        };
        for task_id in &removed {
            if let Some(tid) =
                self.task_for_thread
                    .iter()
                    .find_map(|(&tid, &mapped_task_id)| (mapped_task_id == *task_id).then_some(tid))
            {
                self.remove_task_indexes(pid, *task_id, tid);
            }
        }
        Some(removed)
    }
    /// 批量移除已登记为退出的 member 线程。
    pub fn take_exited_member_tasks(&mut self, pid : ProcessId) -> Option<Vec<TaskId>> {
        let removed = {
            let process = self.process_mut(pid)?;
            let exited = core::mem::take(&mut process.exited_member_task_ids);
            let mut removed = Vec::new();
            for task_id in exited {
                if task_id == process.leader_task_id {
                    continue;
                }
                if process.tasks
                          .get(&task_id)
                          .is_some_and(|task| matches!(task.state, ProcessTaskState::Exited(_)))
                {
                    removed.push(task_id);
                }
            }
            for task_id in &removed {
                process.tasks
                       .remove(task_id);
            }
            removed
        };
        for task_id in &removed {
            if let Some(tid) =
                self.task_for_thread
                    .iter()
                    .find_map(|(&tid, &mapped_task_id)| (mapped_task_id == *task_id).then_some(tid))
            {
                self.remove_task_indexes(pid, *task_id, tid);
            }
        }
        Some(removed)
    }

    /// fork 失败回滚：锁内移除仅含单任务、仍在 Running 的子进程。
    pub(crate) fn detach_aborted_fork(&mut self,
                                      child_task_id : TaskId)
                                      -> ProcessResult<(ProcessId, RetiredProcess)> {
        let pid = self.pid_for_task
                      .get(&child_task_id)
                      .copied()
                      .ok_or(ProcessError::TaskNotFound)?;
        let process = self.processes
                          .get(&pid)
                          .ok_or(ProcessError::ProcessNotFound)?;
        if process.tasks.len() != 1 ||
           !process.tasks
                   .contains_key(&child_task_id)
        {
            return Err(ProcessError::InvalidArgument);
        }
        if !matches!(process.state, ProcessState::Running) {
            return Err(ProcessError::InvalidArgument);
        }
        let process = self.remove_process(pid)
                          .ok_or(ProcessError::ProcessNotFound)?;
        Ok((pid, RetiredProcess { process }))
    }

    /// clone 线程创建失败回滚：从进程线程列表移除该任务（不释放共享地址空间）。
    pub fn abort_cloned_thread(&mut self, child_task_id : TaskId) -> ProcessResult<()> {
        let pid = self.pid_for_task
                      .get(&child_task_id)
                      .copied()
                      .ok_or(ProcessError::TaskNotFound)?;
        let task = {
            let process = self.processes
                              .get_mut(&pid)
                              .ok_or(ProcessError::ProcessNotFound)?;
            if process.leader_task_id == child_task_id {
                return Err(ProcessError::InvalidArgument);
            }
            process.tasks
                   .remove(&child_task_id)
                   .ok_or(ProcessError::TaskNotFound)?
        };
        self.remove_task_indexes(pid, child_task_id, task.tid);
        Ok(())
    }
    // 设置指定 task 的 clear_child_tid 字段，返回是否成功
    pub fn set_task_clear_child_tid(&mut self,
                                    task_id : TaskId,
                                    clear_child_tid : Option<TaskClearTid>)
                                    -> ProcessResult<()> {
        let pid = self.pid_for_task
                      .get(&task_id)
                      .copied()
                      .ok_or(ProcessError::TaskNotFound)?;
        let task = self.processes
                       .get_mut(&pid)
                       .and_then(|process| {
                           process.tasks
                                  .get_mut(&task_id)
                       })
                       .ok_or(ProcessError::TaskNotFound)?;
        task.clear_child_tid = clear_child_tid;
        Ok(())
    }
    // 获取指定 task 的 clear_child_tid 字段，返回 Option<TaskClearTid>
    pub fn task_clear_child_tid(&self, task_id : TaskId) -> Option<TaskClearTid> {
        let pid = self.pid_for_task
                      .get(&task_id)?;
        let process = self.processes
                          .get(pid)?;
        let task = process.tasks
                          .get(&task_id)?;
        task.clear_child_tid
    }
    // 获取指定进程的地址空间，返回 Option<AddressSpaceRef>
    pub fn update_process_address_space(&mut self,
                                        pid : ProcessId,
                                        address_space : Option<AddressSpaceRef>)
                                        -> ProcessResult<()> {
        let process = self.processes
                          .get_mut(&pid)
                          .ok_or(ProcessError::ProcessNotFound)?;
        process.address_space = address_space;
        Ok(())
    }

    pub fn process_snapshot(&self, pid : ProcessId) -> Option<ProcessSnapshot> {
        self.processes
            .get(&pid)
            .map(ProcessControlBlock::snapshot)
    }

    pub fn process_task_snapshot(&self, task_id : TaskId) -> Option<ProcessTaskSnapshot> {
        let pid = self.pid_for_task
                      .get(&task_id)?;
        self.processes
            .get(pid)?
            .ptask_snapshot(task_id)
    }

    pub fn process_identity_for_task(&self,
                                     task_id : TaskId)
                                     -> Option<(ProcessId, Option<ProcessId>)> {
        let pid = *self.pid_for_task
                       .get(&task_id)?;
        let parent_pid = self.processes
                             .get(&pid)?
                             .parent_pid;
        Some((pid, parent_pid))
    }

    pub fn task_id_for_thread(&self, tid : ThreadId) -> Option<TaskId> {
        self.task_for_thread
            .get(&tid)
            .copied()
    }

    pub fn leader_task_for_process(&self, pid : ProcessId) -> Option<TaskId> {
        self.processes
            .get(&pid)
            .map(|process| process.leader_task_id)
    }

    pub fn find_exited_child_process(&self, parent_pid : ProcessId) -> Option<ProcessSnapshot> {
        self.processes
            .values()
            .find(|process| {
                process.parent_pid == Some(parent_pid) &&
                matches!(process.state, ProcessState::Exited(_))
            })
            .map(ProcessControlBlock::snapshot)
    }

    pub fn find_exited_child_process_in_pgid(&self,
                                             parent_pid : ProcessId,
                                             pgid : ProcessId)
                                             -> Option<ProcessSnapshot> {
        self.processes
            .values()
            .find(|process| {
                process.parent_pid == Some(parent_pid) &&
                process.pgid == pgid &&
                matches!(process.state, ProcessState::Exited(_))
            })
            .map(ProcessControlBlock::snapshot)
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

    pub fn mark_process_stopped(&mut self, pid : ProcessId, signo : u8) -> ProcessResult<()> {
        let process = self.process_mut(pid)
                          .ok_or(ProcessError::ProcessNotFound)?;
        if matches!(process.state,
                    ProcessState::Exited(_) | ProcessState::Exiting(_))
        {
            return Err(ProcessError::InvalidArgument);
        }
        if matches!(process.state,
                    ProcessState::Stopped { .. })
        {
            return Ok(());
        }
        process.state = ProcessState::Stopped { signo };
        process.stop_wait_pending = true;
        process.continued_wait_pending = false;
        Ok(())
    }

    pub fn mark_process_continued(&mut self, pid : ProcessId) -> ProcessResult<()> {
        let process = self.process_mut(pid)
                          .ok_or(ProcessError::ProcessNotFound)?;
        if !matches!(process.state,
                     ProcessState::Stopped { .. })
        {
            return Err(ProcessError::InvalidArgument);
        }
        process.state = ProcessState::Running;
        process.continued_wait_pending = true;
        process.stop_wait_pending = false;
        Ok(())
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

    pub fn find_stopped_child_process(&self, parent_pid : ProcessId) -> Option<ProcessSnapshot> {
        self.processes
            .values()
            .find(|process| {
                process.parent_pid == Some(parent_pid) &&
                process.stop_wait_pending &&
                matches!(process.state,
                         ProcessState::Stopped { .. })
            })
            .map(ProcessControlBlock::snapshot)
    }

    pub fn stopped_child_ready_for_wait(&self,
                                        parent_pid : ProcessId,
                                        child_pid : ProcessId)
                                        -> Option<ProcessSnapshot> {
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
        Some(process.snapshot())
    }

    pub fn find_stopped_child_process_in_pgid(&self,
                                              parent_pid : ProcessId,
                                              pgid : ProcessId)
                                              -> Option<ProcessSnapshot> {
        self.processes
            .values()
            .find(|process| {
                process.parent_pid == Some(parent_pid) &&
                process.pgid == pgid &&
                process.stop_wait_pending &&
                matches!(process.state,
                         ProcessState::Stopped { .. })
            })
            .map(ProcessControlBlock::snapshot)
    }

    pub fn find_continued_child_process(&self, parent_pid : ProcessId) -> Option<ProcessSnapshot> {
        self.processes
            .values()
            .find(|process| {
                process.parent_pid == Some(parent_pid) &&
                process.continued_wait_pending &&
                matches!(process.state, ProcessState::Running)
            })
            .map(ProcessControlBlock::snapshot)
    }

    pub fn continued_child_ready_for_wait(&self,
                                          parent_pid : ProcessId,
                                          child_pid : ProcessId)
                                          -> Option<ProcessSnapshot> {
        let process = self.processes
                          .get(&child_pid)?;
        if process.parent_pid != Some(parent_pid) || !process.continued_wait_pending {
            return None;
        }
        if !matches!(process.state, ProcessState::Running) {
            return None;
        }
        Some(process.snapshot())
    }

    pub fn find_continued_child_process_in_pgid(&self,
                                                parent_pid : ProcessId,
                                                pgid : ProcessId)
                                                -> Option<ProcessSnapshot> {
        self.processes
            .values()
            .find(|process| {
                process.parent_pid == Some(parent_pid) &&
                process.pgid == pgid &&
                process.continued_wait_pending &&
                matches!(process.state, ProcessState::Running)
            })
            .map(ProcessControlBlock::snapshot)
    }

    pub fn create_session_for_process(&mut self, pid : ProcessId) -> ProcessResult<()> {
        let process = self.process_mut(pid)
                          .ok_or(ProcessError::ProcessNotFound)?;
        if process.pgid == pid {
            return Err(ProcessError::AlreadySessionLeader);
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

    pub fn set_process_dumpable(&mut self, pid : ProcessId, dumpable : bool) -> ProcessResult<()> {
        let process = self.process_mut(pid)
                          .ok_or(ProcessError::ProcessNotFound)?;
        process.dumpable = dumpable;
        Ok(())
    }

    pub fn process_child_subreaper(&self, pid : ProcessId) -> Option<bool> {
        self.processes
            .get(&pid)
            .map(|process| process.child_subreaper)
    }

    pub fn set_process_child_subreaper(&mut self,
                                       pid : ProcessId,
                                       enabled : bool)
                                       -> ProcessResult<()> {
        let process = self.process_mut(pid)
                          .ok_or(ProcessError::ProcessNotFound)?;
        process.child_subreaper = enabled;
        Ok(())
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

    pub(crate) fn detach_exited_process(&mut self, pid : ProcessId) -> Option<RetiredProcess> {
        let process = self.processes
                          .get(&pid)?;
        if !matches!(process.state, ProcessState::Exited(_)) {
            return None;
        }
        // 托孤：在移除本进程前，将所有子进程的 parent_pid 重定向到 init
        self.reparent_orphans(pid);
        self.remove_process(pid)
            .map(|process| RetiredProcess { process })
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
}

#[cfg(test)]
mod tests {
    use api_v0::{CloneFlags, ProcessId, ProcessState};

    use super::ProcessRegistry;

    #[test]
    fn fork_clears_parent_death_signal() {
        let mut registry = ProcessRegistry::new();
        let parent_pid = registry.create_process_for_task(10, None, None)
                                 .expect("create parent process");
        registry.set_parent_death_signal(parent_pid, 9)
                .expect("set parent death signal");

        let child_pid = registry.create_process_like_fork(parent_pid, 11, None)
                                .expect("fork child process");

        assert_eq!(registry.get_parent_death_signal(parent_pid), Some(9));
        assert_eq!(registry.get_parent_death_signal(child_pid), Some(0));
    }

    #[test]
    fn resolves_process_identity_directly_from_task() {
        let mut registry = ProcessRegistry::new();
        let parent_pid = registry.create_process_for_task(10, None, None)
                                 .expect("create parent process");
        let child_pid = registry.create_process_like_fork(parent_pid, 11, None)
                                .expect("fork child process");

        assert_eq!(registry.process_identity_for_task(11),
                   Some((child_pid, Some(parent_pid))));
        assert_eq!(registry.process_identity_for_task(99), None);
    }

    #[test]
    fn selects_only_members_of_requested_process_group() {
        let mut registry = ProcessRegistry::new();
        let first = registry.create_process_for_task(10, None, None)
                            .expect("create first process");
        let second = registry.create_process_for_task(11, None, None)
                             .expect("create second process");
        let third = registry.create_process_for_task(12, None, None)
                            .expect("create third process");
        let pgid = ProcessId::from_raw(77);
        registry.set_process_pgid(first, pgid)
                .expect("move first process");
        registry.set_process_pgid(third, pgid)
                .expect("move third process");

        assert_eq!(registry.process_pids_in_pgid(pgid),
                   alloc::vec![first, third]);
        assert!(!registry.process_pids_in_pgid(second)
                         .contains(&first));
    }

    #[test]
    fn process_stays_exiting_until_every_thread_exits() {
        let mut registry = ProcessRegistry::new();
        let pid = registry.create_process_for_task(10, None, None)
                          .expect("create process");
        registry.add_task_to_process(pid, 11, CloneFlags::CLONE_THREAD, 0, None)
                .expect("add member thread");

        registry.mark_process_exited(pid, 9)
                .expect("start process exit");
        assert_eq!(registry.process_snapshot(pid).unwrap().state,
                   ProcessState::Exiting(9));
        assert!(registry.detach_exited_process(pid).is_none());

        assert!(!registry.mark_task_exited(10, 9)
                         .expect("exit leader"));
        assert_eq!(registry.process_snapshot(pid).unwrap().state,
                   ProcessState::Exiting(9));
        assert!(registry.detach_exited_process(pid).is_none());

        assert!(registry.mark_task_exited(11, 9)
                        .expect("exit final member"));
        assert_eq!(registry.process_snapshot(pid).unwrap().state,
                   ProcessState::Exited(9));
        assert!(registry.detach_exited_process(pid).is_some());
        assert!(registry.detach_exited_process(pid).is_none());
    }

    #[test]
    fn exec_barrier_rejects_clone_until_thread_group_is_retained() {
        let mut registry = ProcessRegistry::new();
        let pid = registry.create_process_for_task(10, None, None)
                          .expect("create process");
        registry.add_task_to_process(pid, 11, CloneFlags::CLONE_THREAD, 0, None)
                .expect("add existing member");

        assert_eq!(registry.begin_process_exec(pid, 10)
                           .expect("begin exec"),
                   alloc::vec![10, 11]);
        assert!(registry.add_task_to_process(pid, 12, CloneFlags::CLONE_THREAD, 0, None)
                        .is_err());

        registry.retain_only_task_in_process(pid, 10)
                .expect("finish exec");
        registry.add_task_to_process(pid, 13, CloneFlags::CLONE_THREAD, 0, None)
                .expect("clone after exec");
    }
}
