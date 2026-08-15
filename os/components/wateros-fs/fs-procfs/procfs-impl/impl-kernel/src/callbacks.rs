use super::*;

// 本变量代码由AI完成
pub(crate) static ARGV_LOOKUP : Mutex<Option<TaskArgvLookup>> = Mutex::new(None);
// 本变量代码由AI完成
pub(crate) static EXE_LOOKUP : Mutex<Option<TaskExeLookup>> = Mutex::new(None);
pub(crate) static CWD_LOOKUP : Mutex<Option<TaskPathLookup>> = Mutex::new(None);
pub(crate) static ROOT_LOOKUP : Mutex<Option<TaskPathLookup>> = Mutex::new(None);
pub(crate) static FD_LOOKUP : Mutex<Option<TaskFdLookup>> = Mutex::new(None);
pub(crate) static FD_TARGET_LOOKUP : Mutex<Option<TaskFdTargetLookup>> = Mutex::new(None);
pub(crate) static TIMER_SLACK_LOOKUP : Mutex<Option<TaskTimerSlackLookup>> = Mutex::new(None);
// 本变量代码由AI完成
pub(crate) static MOUNT_LOOKUP : Mutex<Option<MountListLookup>> = Mutex::new(None);
pub(crate) static UPTIME_LOOKUP : Mutex<Option<UptimeLookup>> = Mutex::new(None);
pub(crate) static IDLE_TIME_LOOKUP : Mutex<Option<IdleTimeLookup>> = Mutex::new(None);
pub(crate) static SYSVIPC_LOOKUP : Mutex<Option<SysVIpcTableLookup>> = Mutex::new(None);

/// 注册按 leader task id 查询 argv 的回调（VFS 层在 init 时注入）。
// 本方法代码由AI完成
pub fn register_task_argv_lookup(f : TaskArgvLookup) { *ARGV_LOOKUP.lock() = Some(f); }

/// 注册按 leader task id 查询 exe 路径的回调。
// 本方法代码由AI完成
pub fn register_task_exe_lookup(f : TaskExeLookup) { *EXE_LOOKUP.lock() = Some(f); }

/// 注册按 task id 查询 cwd 与进程根目录的回调。
pub fn register_task_cwd_lookup(f : TaskPathLookup) { *CWD_LOOKUP.lock() = Some(f); }
pub fn register_task_root_lookup(f : TaskPathLookup) { *ROOT_LOOKUP.lock() = Some(f); }

/// 注册按 task id 枚举打开 fd 的回调。
pub fn register_task_fd_lookup(f : TaskFdLookup) { *FD_LOOKUP.lock() = Some(f); }

/// 注册 `/proc/<pid>/fd/N` 链接目标查询回调。
pub fn register_task_fd_target_lookup(f : TaskFdTargetLookup) {
    *FD_TARGET_LOOKUP.lock() = Some(f);
}

pub fn register_task_timer_slack_lookup(f : TaskTimerSlackLookup) {
    *TIMER_SLACK_LOOKUP.lock() = Some(f);
}

/// 注册挂载表枚举回调（供 `/proc/mounts`）。
// 本方法代码由AI完成
pub fn register_mount_list_lookup(f : MountListLookup) { *MOUNT_LOOKUP.lock() = Some(f); }

/// 注册内核单调启动时长回调。
pub fn register_uptime_lookup(f : UptimeLookup) { *UPTIME_LOOKUP.lock() = Some(f); }

/// 注册所有 CPU 聚合 idle 时间回调。
pub fn register_idle_time_lookup(f : IdleTimeLookup) { *IDLE_TIME_LOOKUP.lock() = Some(f); }

/// 注册 SysV IPC 注册表快照回调。
pub fn register_sysvipc_table_lookup(f : SysVIpcTableLookup) {
    *SYSVIPC_LOOKUP.lock() = Some(f);
}

// 经静态回调查 argv；未注册时返回 None。
// 本方法代码由AI完成
pub(crate) fn argv_for(leader : TaskId) -> Option<Vec<String>> {
    let lookup = *ARGV_LOOKUP.lock();
    lookup.and_then(|f| f(leader))
}

// 经静态回调查 exe 路径。
// 本方法代码由AI完成
pub(crate) fn exe_for(leader : TaskId) -> Option<String> {
    let lookup = *EXE_LOOKUP.lock();
    lookup.and_then(|f| f(leader))
}

pub(crate) fn cwd_for(leader : TaskId) -> Option<String> {
    (*CWD_LOOKUP.lock()).and_then(|lookup| lookup(leader))
}

pub(crate) fn root_for(leader : TaskId) -> Option<String> {
    (*ROOT_LOOKUP.lock()).and_then(|lookup| lookup(leader))
}

pub(crate) fn thread_comm_str(task_id : TaskId) -> Option<String> {
    let bytes = task::thread_comm(task_id)?;
    let len = bytes.iter()
                   .position(|&b| b == 0)
                   .unwrap_or(bytes.len());
    if len == 0 {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes[..len]).into_owned())
}

pub(crate) fn fds_for(leader : TaskId) -> Vec<usize> {
    let lookup = *FD_LOOKUP.lock();
    lookup.map(|f| f(leader))
          .unwrap_or_default()
}

pub(crate) fn fd_target_for(leader : TaskId, fd : usize) -> Option<String> {
    let lookup = *FD_TARGET_LOOKUP.lock();
    lookup.and_then(|f| f(leader, fd))
}

pub(crate) fn timer_slack_for(leader : TaskId) -> u64 {
    let lookup = *TIMER_SLACK_LOOKUP.lock();
    lookup.map(|f| f(leader))
          .unwrap_or(0)
}

// 经静态回调枚举挂载行；未注册时返回空表。
// 本方法代码由AI完成
pub(crate) fn mount_lines() -> Vec<ProcMountLine> {
    let lookup = *MOUNT_LOOKUP.lock();
    lookup.map(|f| f())
          .unwrap_or_default()
}

pub(crate) fn sysvipc_table(table : SysVIpcTable) -> Vec<u8> {
    let lookup = *SYSVIPC_LOOKUP.lock();
    lookup.map(|f| f(table))
          .unwrap_or_default()
}
