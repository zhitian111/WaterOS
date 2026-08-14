//! 本模块代码由AI完成

//! 内核 procfs：从 task/cred/mm 与 VFS 回调生成 `/proc` 内容。

extern crate alloc;

use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use api_v0::*;
use core::fmt::Write;
use fs_api_v0::{FsAccessMode, FsCapability, FsImpl, FsKind};
use network::{SocketKind, SocketState};
use spin::Mutex;
use task::{ProcessId, ProcessState, TaskState, ThreadId};

// 本变量代码由AI完成
static ARGV_LOOKUP : Mutex<Option<TaskArgvLookup>> = Mutex::new(None);
// 本变量代码由AI完成
static EXE_LOOKUP : Mutex<Option<TaskExeLookup>> = Mutex::new(None);
static FD_LOOKUP : Mutex<Option<TaskFdLookup>> = Mutex::new(None);
static FD_TARGET_LOOKUP : Mutex<Option<TaskFdTargetLookup>> = Mutex::new(None);
static TIMER_SLACK_LOOKUP : Mutex<Option<TaskTimerSlackLookup>> = Mutex::new(None);
// 本变量代码由AI完成
static MOUNT_LOOKUP : Mutex<Option<MountListLookup>> = Mutex::new(None);
static UPTIME_LOOKUP : Mutex<Option<UptimeLookup>> = Mutex::new(None);
static IDLE_TIME_LOOKUP : Mutex<Option<IdleTimeLookup>> = Mutex::new(None);

/// 注册按 leader task id 查询 argv 的回调（VFS 层在 init 时注入）。
// 本方法代码由AI完成
pub fn register_task_argv_lookup(f : TaskArgvLookup) { *ARGV_LOOKUP.lock() = Some(f); }

/// 注册按 leader task id 查询 exe 路径的回调。
// 本方法代码由AI完成
pub fn register_task_exe_lookup(f : TaskExeLookup) { *EXE_LOOKUP.lock() = Some(f); }

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

// 经静态回调查 argv；未注册时返回 None。
// 本方法代码由AI完成
fn argv_for(leader : TaskId) -> Option<Vec<String>> {
    let lookup = *ARGV_LOOKUP.lock();
    lookup.and_then(|f| f(leader))
}

// 经静态回调查 exe 路径。
// 本方法代码由AI完成
fn exe_for(leader : TaskId) -> Option<String> {
    let lookup = *EXE_LOOKUP.lock();
    lookup.and_then(|f| f(leader))
}

fn thread_comm_str(task_id : TaskId) -> Option<String> {
    let bytes = task::thread_comm(task_id)?;
    let len = bytes.iter()
                   .position(|&b| b == 0)
                   .unwrap_or(bytes.len());
    if len == 0 {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes[..len]).into_owned())
}

fn fds_for(leader : TaskId) -> Vec<usize> {
    let lookup = *FD_LOOKUP.lock();
    lookup.map(|f| f(leader))
          .unwrap_or_default()
}

fn fd_target_for(leader : TaskId, fd : usize) -> Option<String> {
    let lookup = *FD_TARGET_LOOKUP.lock();
    lookup.and_then(|f| f(leader, fd))
}

fn timer_slack_for(leader : TaskId) -> u64 {
    let lookup = *TIMER_SLACK_LOOKUP.lock();
    lookup.map(|f| f(leader))
          .unwrap_or(0)
}

// 经静态回调枚举挂载行；未注册时返回空表。
// 本方法代码由AI完成
fn mount_lines() -> Vec<ProcMountLine> {
    let lookup = *MOUNT_LOOKUP.lock();
    lookup.map(|f| f())
          .unwrap_or_default()
}

// 内部路径解析结果；覆盖全局文件与 per-pid 子树。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProcNode {
    Root,
    Meminfo,
    Cpuinfo,
    Uptime,
    Cgroups,
    Mounts,
    NetDir,
    ProcNetTcp,
    ProcNetTcp6,
    ProcNetUdp,
    ProcNetUdp6,
    ProcNetRaw,
    ProcNetRaw6,
    ProcNetUnix,
    SysDir,
    SysKernelDir,
    SysKernelPidMax,
    SysKernelTainted,
    PidDir(ProcessId),
    PidStat(ProcessId),
    PidStatus(ProcessId),
    PidComm(ProcessId),
    PidTimerSlack(ProcessId),
    PidSmaps(ProcessId),
    PidMaps(ProcessId),
    PidCmdline(ProcessId),
    PidExe(ProcessId),
    PidFdDir(ProcessId),
    PidFd(ProcessId, usize),
    PidTaskRoot(ProcessId),
    PidTaskDir(ProcessId, TaskId),
    PidTaskComm(ProcessId, TaskId),
}

// 为 proc 节点分配稳定 inode 号（pid 子树按 pid 编码）。
// 本方法代码由AI完成
fn proc_inode(node : ProcNode) -> u64 {
    match node {
        ProcNode::Root => 1,
        ProcNode::Meminfo => 2,
        ProcNode::Cpuinfo => 7,
        ProcNode::Uptime => 8,
        ProcNode::Cgroups => 6,
        ProcNode::Mounts => 3,
        ProcNode::NetDir => 11,
        ProcNode::ProcNetTcp => 12,
        ProcNode::ProcNetTcp6 => 13,
        ProcNode::ProcNetUdp => 14,
        ProcNode::ProcNetUdp6 => 15,
        ProcNode::ProcNetRaw => 16,
        ProcNode::ProcNetRaw6 => 17,
        ProcNode::ProcNetUnix => 18,
        ProcNode::SysDir => 9,
        ProcNode::SysKernelDir => 10,
        ProcNode::SysKernelPidMax => 4,
        ProcNode::SysKernelTainted => 5,
        ProcNode::PidDir(pid) => 0x1000_0000 | ((pid.raw() as u64) << 4),
        ProcNode::PidStat(pid) => 0x1000_0001 | ((pid.raw() as u64) << 4),
        ProcNode::PidStatus(pid) => 0x1000_0002 | ((pid.raw() as u64) << 4),
        ProcNode::PidComm(pid) => 0x1000_0009 | ((pid.raw() as u64) << 4),
        ProcNode::PidTimerSlack(pid) => 0x1000_000a | ((pid.raw() as u64) << 4),
        ProcNode::PidSmaps(pid) => 0x1000_0003 | ((pid.raw() as u64) << 4),
        ProcNode::PidMaps(pid) => 0x1000_0005 | ((pid.raw() as u64) << 4),
        ProcNode::PidCmdline(pid) => 0x1000_0004 | ((pid.raw() as u64) << 4),
        ProcNode::PidExe(pid) => 0x1000_0006 | ((pid.raw() as u64) << 4),
        ProcNode::PidFdDir(pid) => 0x1000_0007 | ((pid.raw() as u64) << 4),
        ProcNode::PidTaskRoot(pid) => 0x1000_0008 | ((pid.raw() as u64) << 4),
        ProcNode::PidTaskDir(pid, tid) => {
            0x3000_0000_0000_0000 | ((pid.raw() as u64) << 32) | (tid as u64)
        }
        ProcNode::PidTaskComm(pid, tid) => {
            0x3000_0000_0000_0000 | (1u64 << 60) | ((pid.raw() as u64) << 32) | (tid as u64)
        }
        ProcNode::PidFd(pid, fd) => {
            0x2000_0000_0000_0000 | ((pid.raw() as u64) << 32) | fd as u64
        }
    }
}

/// 与 VFS `normalize_absolute_path` 一致：折叠 `//`、`.`，解析 `..`。
// 本方法代码由AI完成
fn normalize_rel(path : &str) -> String {
    use alloc::borrow::Cow;

    let abs : Cow<'_, str> = if path.is_empty() {
        Cow::Borrowed("/")
    } else if path.starts_with('/') {
        Cow::Borrowed(path)
    } else {
        Cow::Owned(format!("/{path}"))
    };
    let mut parts : Vec<&str> = Vec::new();
    for part in abs.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            let _ = parts.pop();
            continue;
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return String::from("/");
    }
    let mut out = String::with_capacity(abs.len());
    out.push('/');
    for (i, p) in parts.iter()
                       .enumerate()
    {
        if i > 0 {
            out.push('/');
        }
        out.push_str(p);
    }
    out
}

// 解析 pid 目录名：`self` 映射当前进程，否则按十进制 pid。
// 本方法代码由AI完成
fn parse_pid(name : &str) -> Option<ProcessId> {
    if name == "self" {
        Some(task::current_process_task_snapshot()?.pid)
    } else {
        Some(ProcessId::from_raw(name.parse::<usize>()
                                     .ok()?))
    }
}

fn parse_thread_task(pid : ProcessId, name : &str) -> Option<TaskId> {
    let tid = name.parse::<usize>().ok()?;
    let task_id = task::task_id_for_thread(ThreadId::from_raw(tid))?;
    task::task_ids_for_process(pid)?.contains(&task_id).then_some(task_id)
}

// 将相对 `/proc` 的路径映射为内部节点；未知路径返回 None。
// 本方法代码由AI完成
fn parse_node(path : &str) -> Option<ProcNode> {
    let p = normalize_rel(path);
    if p == "/" {
        return Some(ProcNode::Root);
    }
    let rest = p.strip_prefix('/')?;
    let comps : Vec<&str> = rest.split('/')
                                .collect();
    match comps.as_slice() {
        ["meminfo"] => Some(ProcNode::Meminfo),
        ["cpuinfo"] => Some(ProcNode::Cpuinfo),
        ["uptime"] => Some(ProcNode::Uptime),
        ["cgroups"] => Some(ProcNode::Cgroups),
        ["mounts"] => Some(ProcNode::Mounts),
        ["net"] => Some(ProcNode::NetDir),
        ["net", "tcp"] => Some(ProcNode::ProcNetTcp),
        ["net", "tcp6"] => Some(ProcNode::ProcNetTcp6),
        ["net", "udp"] => Some(ProcNode::ProcNetUdp),
        ["net", "udp6"] => Some(ProcNode::ProcNetUdp6),
        ["net", "raw"] => Some(ProcNode::ProcNetRaw),
        ["net", "raw6"] => Some(ProcNode::ProcNetRaw6),
        ["net", "unix"] => Some(ProcNode::ProcNetUnix),
        ["sys"] => Some(ProcNode::SysDir),
        ["sys", "kernel"] => Some(ProcNode::SysKernelDir),
        ["sys", "kernel", "pid_max"] => Some(ProcNode::SysKernelPidMax),
        ["sys", "kernel", "tainted"] => Some(ProcNode::SysKernelTainted),
        [pid_name] => Some(ProcNode::PidDir(parse_pid(pid_name)?)),
        [pid_name, "stat"] => Some(ProcNode::PidStat(parse_pid(pid_name)?)),
        [pid_name, "status"] => Some(ProcNode::PidStatus(parse_pid(pid_name)?)),
        [pid_name, "comm"] => Some(ProcNode::PidComm(parse_pid(pid_name)?)),
        [pid_name, "timerslack_ns"] => Some(ProcNode::PidTimerSlack(parse_pid(pid_name)?)),
        [pid_name, "smaps"] => Some(ProcNode::PidSmaps(parse_pid(pid_name)?)),
        [pid_name, "maps"] => Some(ProcNode::PidMaps(parse_pid(pid_name)?)),
        [_pid_name, "mounts"] => Some(ProcNode::Mounts),
        [pid_name, "cmdline"] => Some(ProcNode::PidCmdline(parse_pid(pid_name)?)),
        [pid_name, "exe"] => Some(ProcNode::PidExe(parse_pid(pid_name)?)),
        [pid_name, "fd"] => Some(ProcNode::PidFdDir(parse_pid(pid_name)?)),
        [pid_name, "task"] => Some(ProcNode::PidTaskRoot(parse_pid(pid_name)?)),
        [pid_name, "fd", fd] => {
            Some(ProcNode::PidFd(parse_pid(pid_name)?, fd.parse().ok()?))
        }
        [pid_name, "task", tid_name] => {
            let pid = parse_pid(pid_name)?;
            Some(ProcNode::PidTaskDir(pid, parse_thread_task(pid, tid_name)?))
        }
        [pid_name, "task", tid_name, "comm"] => {
            let pid = parse_pid(pid_name)?;
            Some(ProcNode::PidTaskComm(pid, parse_thread_task(pid, tid_name)?))
        }
        _ => None,
    }
}

#[path = "render.rs"]
mod render;
use render::*;

/// 内核 procfs 只读视图（零大小；无实例状态）。
// 本结构代码由AI完成
pub struct KernelProcFs;

/// 返回全局 procfs 视图句柄。
pub fn view() -> &'static KernelProcFs { &KernelProcFs }

impl ProcFsView for KernelProcFs {
    // 本方法代码由AI完成
    fn exists(&self, rel_path : &str) -> FsResult<bool> {
        let Some(node) = parse_node(rel_path) else {
            return Ok(false);
        };
        Ok(match node {
            ProcNode::Root |
            ProcNode::Meminfo |
            ProcNode::Cpuinfo |
            ProcNode::Uptime |
            ProcNode::Cgroups |
            ProcNode::Mounts |
            ProcNode::NetDir |
            ProcNode::SysDir |
            ProcNode::SysKernelDir => true,
            ProcNode::ProcNetTcp |
            ProcNode::ProcNetTcp6 |
            ProcNode::ProcNetUdp |
            ProcNode::ProcNetUdp6 |
            ProcNode::ProcNetRaw |
            ProcNode::ProcNetRaw6 |
            ProcNode::ProcNetUnix => true,
            ProcNode::SysKernelPidMax | ProcNode::SysKernelTainted => true,
            ProcNode::PidDir(pid) |
            ProcNode::PidStat(pid) |
            ProcNode::PidStatus(pid) |
            ProcNode::PidComm(pid) |
            ProcNode::PidTimerSlack(pid) |
            ProcNode::PidSmaps(pid) |
            ProcNode::PidMaps(pid) |
            ProcNode::PidCmdline(pid) |
            ProcNode::PidExe(pid) |
            ProcNode::PidFdDir(pid) |
            ProcNode::PidTaskRoot(pid) => process_visible(pid),
            ProcNode::PidTaskDir(pid, _) | ProcNode::PidTaskComm(pid, _) => process_visible(pid),
            ProcNode::PidFd(pid, fd) => {
                task::leader_task_for_process(pid)
                    .map(|leader| fds_for(leader).contains(&fd))
                    .unwrap_or(false)
            }
        })
    }

    // 本方法代码由AI完成
    fn metadata(&self, rel_path : &str) -> FsResult<FsMetadata> {
        let node = parse_node(rel_path).ok_or(FsError::NotFound)?;
        match node {
            ProcNode::Root |
            ProcNode::NetDir |
            ProcNode::SysDir |
            ProcNode::SysKernelDir |
            ProcNode::PidDir(_) |
            ProcNode::PidFdDir(_) |
            ProcNode::PidTaskRoot(_) |
            ProcNode::PidTaskDir(_, _) => Ok(FsMetadata { node_type : FsNodeType::Directory,
                                                   size : 0,
                                                   mode : 0o555,
                                                   inode : proc_inode(node),
                                                   nlink : 1,
                                                   uid : 0,
                                                   gid : 0 }),
            ProcNode::Meminfo |
            ProcNode::Cpuinfo |
            ProcNode::Uptime |
            ProcNode::Cgroups |
            ProcNode::Mounts |
            ProcNode::ProcNetTcp |
            ProcNode::ProcNetTcp6 |
            ProcNode::ProcNetUdp |
            ProcNode::ProcNetUdp6 |
            ProcNode::ProcNetRaw |
            ProcNode::ProcNetRaw6 |
            ProcNode::ProcNetUnix |
            ProcNode::SysKernelPidMax |
            ProcNode::SysKernelTainted => Ok(FsMetadata { node_type : FsNodeType::File,
                                                          size : self.read(rel_path)?
                                                             .len()
                                                                 as u64,
                                                          mode : 0o444,
                                                          inode : proc_inode(node),
                                                          nlink : 1,
                                                          uid : 0,
                                                          gid : 0 }),
            ProcNode::PidStat(pid) |
            ProcNode::PidStatus(pid) |
            ProcNode::PidComm(pid) |
            ProcNode::PidTimerSlack(pid) |
            ProcNode::PidSmaps(pid) |
            ProcNode::PidMaps(pid) |
            ProcNode::PidCmdline(pid) |
            ProcNode::PidTaskComm(pid, _) => {
                if !process_visible(pid) {
                    return Err(FsError::NotFound);
                }
                Ok(FsMetadata { node_type : FsNodeType::File,
                                size : self.read(rel_path)?
                                           .len() as u64,
                                mode : 0o444,
                                inode : proc_inode(node),
                                nlink : 1,
                                uid : 0,
                                gid : 0 })
            }
            ProcNode::PidExe(pid) | ProcNode::PidFd(pid, _) => {
                if !process_visible(pid) {
                    return Err(FsError::NotFound);
                }
                Ok(FsMetadata { node_type: FsNodeType::Symlink,
                                size: self.read_symlink(rel_path)?.len() as u64,
                                mode: 0o777,
                                inode: proc_inode(node),
                                nlink: 1,
                                uid: 0,
                                gid: 0 })
            }
        }
    }

    // 本方法代码由AI完成
    fn read(&self, rel_path : &str) -> FsResult<Vec<u8>> {
        let node = parse_node(rel_path).ok_or(FsError::NotFound)?;
        match node {
            ProcNode::Root |
            ProcNode::SysDir |
            ProcNode::SysKernelDir |
            ProcNode::PidDir(_) |
            ProcNode::PidFdDir(_) |
            ProcNode::PidTaskRoot(_) |
            ProcNode::PidTaskDir(_, _) |
            ProcNode::NetDir |
            ProcNode::PidExe(_) |
            ProcNode::PidFd(_, _) => {
                Err(FsError::NotAFile)
            }
            ProcNode::ProcNetTcp => Ok(format_proc_net_table(SocketKind::Tcp)),
            ProcNode::ProcNetUdp => Ok(format_proc_net_table(SocketKind::Udp)),
            ProcNode::ProcNetTcp6 |
            ProcNode::ProcNetUdp6 |
            ProcNode::ProcNetRaw |
            ProcNode::ProcNetRaw6 => Ok(PROC_NET_TABLE.to_vec()),
            ProcNode::ProcNetUnix => Ok(PROC_NET_UNIX_TABLE.to_vec()),
            ProcNode::Meminfo => Ok(format_meminfo()),
            ProcNode::Cpuinfo => Ok(format_cpuinfo()),
            ProcNode::Uptime => Ok(format_uptime()),
            ProcNode::Cgroups => Ok(format_cgroups()),
            ProcNode::Mounts => Ok(format_mounts()),
            ProcNode::SysKernelPidMax => Ok(b"32768\n".to_vec()),
            ProcNode::SysKernelTainted => Ok(b"0\n".to_vec()),
            ProcNode::PidStat(pid) => format_stat(pid),
            ProcNode::PidStatus(pid) => format_status(pid),
            ProcNode::PidComm(pid) => format_pid_comm(pid),
            ProcNode::PidTimerSlack(pid) => format_pid_timer_slack(pid),
            ProcNode::PidSmaps(pid) => format_smaps(pid),
            ProcNode::PidMaps(pid) => format_maps(pid),
            ProcNode::PidCmdline(pid) => format_cmdline(pid),
            ProcNode::PidTaskComm(pid, task_id) => format_task_comm(pid, task_id),
        }
    }

    fn read_range(&self, rel_path : &str, offset : u64, buf : &mut [u8]) -> FsResult<usize> {
        let node = parse_node(rel_path).ok_or(FsError::NotFound)?;
        let static_data : &[u8] = match node {
            ProcNode::ProcNetTcp6 |
            ProcNode::ProcNetUdp6 => PROC_NET_TABLE,
            ProcNode::ProcNetRaw |
            ProcNode::ProcNetRaw6 => PROC_NET_TABLE,
            ProcNode::ProcNetUnix => PROC_NET_UNIX_TABLE,
            ProcNode::ProcNetTcp |
            ProcNode::ProcNetUdp => {
                let data = self.read(rel_path)?;
                let start = offset as usize;
                if start >= data.len() {
                    return Ok(0);
                }
                let n = core::cmp::min(buf.len(), data.len() - start);
                buf[..n].copy_from_slice(&data[start..start + n]);
                return Ok(n);
            }
            ProcNode::SysKernelPidMax => b"32768\n",
            ProcNode::SysKernelTainted => b"0\n",
            _ => return ProcFsView::read_range(self, rel_path, offset, buf),
        };
        let start = offset as usize;
        if start >= static_data.len() {
            return Ok(0);
        }
        let n = core::cmp::min(buf.len(), static_data.len() - start);
        buf[..n].copy_from_slice(&static_data[start..start + n]);
        Ok(n)
    }

    fn read_symlink(&self, rel_path : &str) -> FsResult<Vec<u8>> {
        let node = parse_node(rel_path).ok_or(FsError::NotFound)?;
        match node {
            ProcNode::PidExe(pid) => {
                let process = task::process_snapshot(pid).ok_or(FsError::NotFound)?;
                exe_for(process.leader_task_id)
                    .map(String::into_bytes)
                    .ok_or(FsError::NotFound)
            }
            ProcNode::PidFd(pid, fd) => {
                let leader = task::leader_task_for_process(pid).ok_or(FsError::NotFound)?;
                if !fds_for(leader).contains(&fd) {
                    return Err(FsError::NotFound);
                }
                Ok(fd_target_for(leader, fd)
                    .unwrap_or_else(|| format!("anon_inode:[wateros-fd-{fd}]"))
                    .into_bytes())
            }
            _ => Err(FsError::NotAFile),
        }
    }

    // 本方法代码由AI完成
    fn read_dir(&self, rel_path : &str) -> FsResult<Vec<FsDirEntry>> {
        let node = parse_node(rel_path).ok_or(FsError::NotFound)?;
        match node {
            ProcNode::Root => {
                let mut entries = vec![FsDirEntry { name : String::from("meminfo"),
                                                    node_type : FsNodeType::File },
                                       FsDirEntry { name : String::from("cpuinfo"),
                                                    node_type : FsNodeType::File },
                                       FsDirEntry { name : String::from("uptime"),
                                                    node_type : FsNodeType::File },
                                       FsDirEntry { name : String::from("cgroups"),
                                                    node_type : FsNodeType::File },
                                       FsDirEntry { name : String::from("mounts"),
                                                    node_type : FsNodeType::File },
                                       FsDirEntry { name : String::from("net"),
                                                    node_type : FsNodeType::Directory },
                                       FsDirEntry { name : String::from("sys"),
                                                    node_type : FsNodeType::Directory },];
                for pid in task::all_process_pids() {
                    entries.push(FsDirEntry { name : format!("{}", pid.raw()),
                                              node_type : FsNodeType::Directory });
                }
                Ok(entries)
            }
            ProcNode::SysDir => Ok(vec![FsDirEntry { name : String::from("kernel"),
                                                     node_type : FsNodeType::Directory }]),
            ProcNode::SysKernelDir => {
                Ok(vec![FsDirEntry { name : String::from("pid_max"),
                                     node_type : FsNodeType::File },
                        FsDirEntry { name : String::from("tainted"),
                                     node_type : FsNodeType::File }])
            }
            ProcNode::NetDir => Ok(vec![
                FsDirEntry { name : String::from("tcp"),
                             node_type : FsNodeType::File },
                FsDirEntry { name : String::from("tcp6"),
                             node_type : FsNodeType::File },
                FsDirEntry { name : String::from("udp"),
                             node_type : FsNodeType::File },
                FsDirEntry { name : String::from("udp6"),
                             node_type : FsNodeType::File },
                FsDirEntry { name : String::from("raw"),
                             node_type : FsNodeType::File },
                FsDirEntry { name : String::from("raw6"),
                             node_type : FsNodeType::File },
                FsDirEntry { name : String::from("unix"),
                             node_type : FsNodeType::File },
            ]),
            ProcNode::PidDir(pid) => {
                if !process_visible(pid) {
                    return Err(FsError::NotFound);
                }
                Ok(vec![FsDirEntry { name:
                                         String::from("stat"),
                                     node_type:
                                         FsNodeType::File },
                        FsDirEntry { name:
                                         String::from("status"),
                                     node_type:
                                         FsNodeType::File },
                        FsDirEntry { name:
                                         String::from("comm"),
                                     node_type:
                                         FsNodeType::File },
                        FsDirEntry { name:
                                         String::from("timerslack_ns"),
                                     node_type:
                                         FsNodeType::File },
                        FsDirEntry { name:
                                         String::from("smaps"),
                                     node_type:
                                         FsNodeType::File },
                        FsDirEntry { name:
                                         String::from("maps"),
                                     node_type:
                                         FsNodeType::File },
                        FsDirEntry { name:
                                         String::from("mounts"),
                                     node_type:
                                         FsNodeType::File },
                        FsDirEntry { name:
                                         String::from("cmdline"),
                                     node_type:
                                         FsNodeType::File },
                        FsDirEntry { name:
                                         String::from("exe"),
                                     node_type:
                                         FsNodeType::Symlink },
                        FsDirEntry { name:
                                         String::from("fd"),
                                     node_type:
                                         FsNodeType::Directory },
                        FsDirEntry { name:
                                         String::from("task"),
                                     node_type:
                                         FsNodeType::Directory },])
            }
            ProcNode::PidFdDir(pid) => {
                let leader = task::leader_task_for_process(pid).ok_or(FsError::NotFound)?;
                Ok(fds_for(leader)
                    .into_iter()
                    .map(|fd| FsDirEntry { name : fd.to_string(),
                                           node_type : FsNodeType::Symlink })
                    .collect())
            }
            ProcNode::PidTaskRoot(pid) => {
                if !process_visible(pid) {
                    return Err(FsError::NotFound);
                }
                Ok(task::task_ids_for_process(pid)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|task_id| {
                        let tid = task::process_task_snapshot(task_id)
                                      .map(|snap| snap.tid.raw())
                                      .unwrap_or(task_id);
                        FsDirEntry { name : tid.to_string(),
                                     node_type : FsNodeType::Directory }
                    })
                    .collect())
            }
            ProcNode::PidTaskDir(pid, _) => {
                if !process_visible(pid) {
                    return Err(FsError::NotFound);
                }
                Ok(vec![FsDirEntry { name : String::from("comm"),
                                     node_type : FsNodeType::File }])
            }
            _ => Err(FsError::NotAFile),
        }
    }
}

/// procfs 的 [`FsImpl`] 注册项；仅列能力，不参与块卷挂载。
// 本结构代码由AI完成
pub struct KernelProcFsImpl;

/// 全局 procfs impl 实例。
// 本变量代码由AI完成
pub static IMPL : KernelProcFsImpl = KernelProcFsImpl;

// 本变量代码由AI完成
const SUPPORTED : &[FsCapability] = &[FsCapability::new(FsKind::Other("procfs"),
                                                        FsAccessMode::ReadOnly)];

impl FsImpl for KernelProcFsImpl {
    fn name(&self) -> &'static str { "procfs" }

    fn supported(&self) -> &'static [FsCapability] { SUPPORTED }

    // 本方法代码由AI完成
    fn mount_ro(&self,
                _device : driver_block_api_v0::SharedBlockDevice)
                -> fs_api_v0::FsResult<fs_api_v0::SharedFs> {
        Err(FsError::Unsupported)
    }
}

/// 最小自检：枚举根目录并打日志。
// 本方法代码由AI完成
pub fn test() {
    let v = view();
    let _ = v.read_dir("/");
    logging::info!("[fs::procfs] self_test ok");
}

#[cfg(feature = "self_test")]
pub fn self_test() {
    test();
}

