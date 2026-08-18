//! procfs 与 task、VFS 等上层子系统之间的只读回调注册表。
//!
//! 每项回调在 init 阶段注册，读取时先复制函数指针再释放锁，避免回调反向进入 procfs 时死锁。

use super::*;

// 本变量代码由AI完成
/// exec 参数、环境、auxv 和 I/O 统计的查询入口；未注册时对应文件以空内容或默认值降级。
pub(crate) static ARGV_LOOKUP : Mutex<Option<TaskArgvLookup>> = Mutex::new(None);
pub(crate) static ENV_LOOKUP : Mutex<Option<TaskEnvLookup>> = Mutex::new(None);
pub(crate) static AUXV_LOOKUP : Mutex<Option<TaskAuxvLookup>> = Mutex::new(None);
pub(crate) static IO_LOOKUP : Mutex<Option<TaskIoLookup>> = Mutex::new(None);
// 本变量代码由AI完成
/// 可执行文件、cwd、根目录和 fd 信息的查询入口。
pub(crate) static EXE_LOOKUP : Mutex<Option<TaskExeLookup>> = Mutex::new(None);
pub(crate) static CWD_LOOKUP : Mutex<Option<TaskPathLookup>> = Mutex::new(None);
pub(crate) static ROOT_LOOKUP : Mutex<Option<TaskPathLookup>> = Mutex::new(None);
pub(crate) static FD_LOOKUP : Mutex<Option<TaskFdLookup>> = Mutex::new(None);
pub(crate) static FD_TARGET_LOOKUP : Mutex<Option<TaskFdTargetLookup>> = Mutex::new(None);
pub(crate) static TIMER_SLACK_LOOKUP : Mutex<Option<TaskTimerSlackLookup>> = Mutex::new(None);
// 本变量代码由AI完成
/// 挂载、时钟和 SysV IPC 快照的查询入口。
pub(crate) static MOUNT_LOOKUP : Mutex<Option<MountListLookup>> = Mutex::new(None);
pub(crate) static UPTIME_LOOKUP : Mutex<Option<UptimeLookup>> = Mutex::new(None);
pub(crate) static IDLE_TIME_LOOKUP : Mutex<Option<IdleTimeLookup>> = Mutex::new(None);
pub(crate) static SYSVIPC_LOOKUP : Mutex<Option<SysVIpcTableLookup>> = Mutex::new(None);

/// 注册按 leader task id 查询 argv 的回调（VFS 层在 init 时注入）。
// 本方法代码由AI完成
///
/// 同类回调后注册者会覆盖前者，故只能在单一的系统初始化所有者处调用。
pub fn register_task_argv_lookup(f : TaskArgvLookup) { *ARGV_LOOKUP.lock() = Some(f); }

/// 注册 exec 环境向量查询回调；未注册时 `/proc/<pid>/environ` 为空。
pub fn register_task_env_lookup(f : TaskEnvLookup) { *ENV_LOOKUP.lock() = Some(f); }
/// 注册 auxv 原始字节查询回调；调用者须保留用户 ABI 的字宽和字节序。
pub fn register_task_auxv_lookup(f : TaskAuxvLookup) { *AUXV_LOOKUP.lock() = Some(f); }
/// 注册字符 I/O 统计查询回调；没有可靠计数时回调可返回 `None`。
pub fn register_task_io_lookup(f : TaskIoLookup) { *IO_LOOKUP.lock() = Some(f); }

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

/// 注册 timer slack 查询回调；返回值的单位为纳秒。
pub fn register_task_timer_slack_lookup(f : TaskTimerSlackLookup) {
    *TIMER_SLACK_LOOKUP.lock() = Some(f);
}

// 本方法代码由AI完成
/// 注册挂载表枚举回调（供 `/proc/mounts`）；回调应自行产出一致快照。
pub fn register_mount_list_lookup(f : MountListLookup) { *MOUNT_LOOKUP.lock() = Some(f); }

/// 注册内核单调启动时长回调。
pub fn register_uptime_lookup(f : UptimeLookup) { *UPTIME_LOOKUP.lock() = Some(f); }

/// 注册所有 CPU 聚合 idle 时间回调。
pub fn register_idle_time_lookup(f : IdleTimeLookup) { *IDLE_TIME_LOOKUP.lock() = Some(f); }

/// 注册 SysV IPC 注册表快照回调。
pub fn register_sysvipc_table_lookup(f : SysVIpcTableLookup) {
    *SYSVIPC_LOOKUP.lock() = Some(f);
}

// 本方法代码由AI完成
/// 经已注册回调查 argv；复制指针后在锁外执行，未注册或任务不存在时返回 `None`。
pub(crate) fn argv_for(leader : TaskId) -> Option<Vec<String>> {
    let lookup = *ARGV_LOOKUP.lock();
    lookup.and_then(|f| f(leader))
}

pub(crate) fn env_for(leader : TaskId) -> Option<Vec<String>> {
    let lookup = *ENV_LOOKUP.lock();
    lookup.and_then(|f| f(leader))
}

pub(crate) fn auxv_for(leader : TaskId) -> Option<Vec<u8>> {
    let lookup = *AUXV_LOOKUP.lock();
    lookup.and_then(|f| f(leader))
}

pub(crate) fn io_for(leader : TaskId) -> Option<[u64; 4]> {
    let lookup = *IO_LOOKUP.lock();
    lookup.and_then(|f| f(leader))
}

// 本方法代码由AI完成
/// 查询 exe 路径；进程尚未 exec 或已退出时返回 `None`。
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

// 本方法代码由AI完成
/// 枚举挂载行；未注册时返回空表而不是使 procfs 读取失败。
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
