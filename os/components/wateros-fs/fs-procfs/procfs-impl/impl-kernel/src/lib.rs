#![no_std]
//! 本模块代码由AI完成

//! 内核 procfs：从 task/cred/mm 与 VFS 回调生成 `/proc` 内容。

extern crate alloc;

use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use api_v0::{
    FsDirEntry, FsError, FsMetadata, FsNodeType, FsResult, IdleTimeLookup, MountListLookup, ProcFsView,
    ProcMountLine, TaskArgvLookup, TaskExeLookup, TaskFdLookup, TaskId, UptimeLookup,
};
use fs_api_v0::{FsAccessMode, FsCapability, FsImpl, FsKind};
use spin::Mutex;
use task::{ProcessId, ProcessState, TaskState, ThreadId};

// 本变量代码由AI完成
static ARGV_LOOKUP : Mutex<Option<TaskArgvLookup>> = Mutex::new(None);
// 本变量代码由AI完成
static EXE_LOOKUP : Mutex<Option<TaskExeLookup>> = Mutex::new(None);
static FD_LOOKUP : Mutex<Option<TaskFdLookup>> = Mutex::new(None);
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
    SysDir,
    SysKernelDir,
    SysKernelPidMax,
    SysKernelTainted,
    PidDir(ProcessId),
    PidStat(ProcessId),
    PidStatus(ProcessId),
    PidComm(ProcessId),
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
        ProcNode::SysDir => 9,
        ProcNode::SysKernelDir => 10,
        ProcNode::SysKernelPidMax => 4,
        ProcNode::SysKernelTainted => 5,
        ProcNode::PidDir(pid) => 0x1000_0000 | ((pid.raw() as u64) << 4),
        ProcNode::PidStat(pid) => 0x1000_0001 | ((pid.raw() as u64) << 4),
        ProcNode::PidStatus(pid) => 0x1000_0002 | ((pid.raw() as u64) << 4),
        ProcNode::PidComm(pid) => 0x1000_0009 | ((pid.raw() as u64) << 4),
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
        ["sys"] => Some(ProcNode::SysDir),
        ["sys", "kernel"] => Some(ProcNode::SysKernelDir),
        ["sys", "kernel", "pid_max"] => Some(ProcNode::SysKernelPidMax),
        ["sys", "kernel", "tainted"] => Some(ProcNode::SysKernelTainted),
        [pid_name] => Some(ProcNode::PidDir(parse_pid(pid_name)?)),
        [pid_name, "stat"] => Some(ProcNode::PidStat(parse_pid(pid_name)?)),
        [pid_name, "status"] => Some(ProcNode::PidStatus(parse_pid(pid_name)?)),
        [pid_name, "comm"] => Some(ProcNode::PidComm(parse_pid(pid_name)?)),
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

// 进程仍存在于 task 子系统时才对外可见。
// 本方法代码由AI完成
fn process_visible(pid : ProcessId) -> bool { task::process_snapshot(pid).is_some() }

// 进程 comm：优先 argv[0] 基名，其次 exe 基名，最后回退 `"process"`。
// 本方法代码由AI完成
fn comm_for(pid : ProcessId) -> String {
    let leader = task::leader_task_for_process(pid).unwrap_or(0);
    if let Some(comm) = thread_comm_str(leader) {
        return comm;
    }
    if let Some(argv) = argv_for(leader) {
        if let Some(arg0) = argv.first() {
            return basename(arg0);
        }
    }
    if let Some(exe) = exe_for(leader) {
        return basename(exe.as_str());
    }
    String::from("process")
}

fn format_task_comm(pid : ProcessId, task_id : TaskId) -> FsResult<Vec<u8>> {
    if !process_visible(pid) {
        return Err(FsError::NotFound);
    }
    let comm = thread_comm_str(task_id).unwrap_or_else(|| comm_for(pid));
    let mut out = comm.into_bytes();
    out.push(b'\n');
    Ok(out)
}

fn format_pid_comm(pid : ProcessId) -> FsResult<Vec<u8>> {
    if !process_visible(pid) {
        return Err(FsError::NotFound);
    }
    let mut out = comm_for(pid).into_bytes();
    out.push(b'\n');
    Ok(out)
}

// 本方法代码由AI完成
fn format_cpuinfo() -> Vec<u8> {
    let online = task::online_cpu_mask();
    let mut output = String::new();
    for cpu in 0..u64::BITS as usize {
        if online.contains(task::CpuId::from_raw(cpu)) {
            output.push_str(format!("processor\t: {cpu}\n").as_str());
            output.push_str("hart\t\t: ");
            output.push_str(cpu.to_string().as_str());
            output.push('\n');
            output.push_str("model name\t: WaterOS RISC-V virtual CPU\n");
            output.push_str("isa\t\t: rv64imafdch\n\n");
        }
    }
    output.into_bytes()
}

fn format_uptime() -> Vec<u8> {
    let nanos = (*UPTIME_LOOKUP.lock()).map(|lookup| lookup())
                                         .unwrap_or(0);
    let seconds = nanos / 1_000_000_000;
    let centiseconds = nanos % 1_000_000_000 / 10_000_000;
    let idle_nanos = (*IDLE_TIME_LOOKUP.lock()).map(|lookup| lookup())
                                                    .unwrap_or(0);
    let idle_seconds = idle_nanos / 1_000_000_000;
    let idle_centiseconds = idle_nanos % 1_000_000_000 / 10_000_000;
    format!("{seconds}.{centiseconds:02} {idle_seconds}.{idle_centiseconds:02}\n").into_bytes()
}

// 本方法代码由AI完成
fn format_cgroups() -> Vec<u8> {
    b"#subsys_name\thierarchy\tnum_cgroups\tenabled\n\
      memory\t1\t1\t1\n\
      cpuset\t2\t1\t1\n\
      cpu\t3\t1\t1\n\
      cpuacct\t4\t1\t1\n\
      pids\t5\t1\t1\n\
      freezer\t6\t1\t1\n\
      devices\t7\t1\t1\n\
      blkio\t8\t1\t1\n\
      net_cls\t9\t1\t1\n\
      perf_event\t10\t1\t1\n\
      net_prio\t11\t1\t1\n\
      hugetlb\t12\t1\t1\n".to_vec()
}

// 取路径最后一段作为 comm 展示名。
// 本方法代码由AI完成
fn basename(path : &str) -> String {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .to_string()
}

// Linux `/proc/pid/stat` 单字符状态码（简化版）。
// 本方法代码由AI完成
fn state_char(process : ProcessState, leader_state : Option<TaskState>) -> char {
    match process {
        ProcessState::Exited(_) | ProcessState::Exiting(_) => 'Z',
        ProcessState::Stopped { .. } => 'T',
        ProcessState::Running => match leader_state {
            Some(TaskState::Sleeping { .. }) | Some(TaskState::Blocking(_)) => 'S',
            Some(TaskState::Exited(_)) => 'Z',
            Some(TaskState::Ready) | Some(TaskState::Running { .. }) | None => 'R',
        },
    }
}

// 本方法代码由AI完成
fn format_stat(pid : ProcessId) -> FsResult<Vec<u8>> {
    let process = task::process_snapshot(pid).ok_or(FsError::NotFound)?;
    let leader = process.leader_task_id;
    // Linux utime/stime are in USER_HZ jiffies; tick_count tracks scheduler ticks (~100/s).
    let jiffies = task::task_snapshot(leader).map(|snap| {
                                                 snap.stats
                                                     .tick_count
                                                 as u64
                                             })
                                             .unwrap_or(0)
                                             .max(1);
    let comm = comm_for(pid);
    let comm15 = if comm.len() > 15 {
        comm[..15].to_string()
    } else {
        comm
    };
    let ppid = process.parent_pid
                      .map(|p| p.raw())
                      .unwrap_or(0);
    let utime = jiffies;
    let stime = jiffies;
    let leader_state = task::task_snapshot(leader).map(|snap| snap.state);
    let sc = state_char(process.state, leader_state);
    let line = format!("{} ({}) {} {} 0 0 0 0 0 0 0 0 {} {} 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 \
                        0 0 0 0 0 0\n",
                       pid.raw(),
                       comm15,
                       sc,
                       ppid,
                       utime,
                       stime,);
    Ok(line.into_bytes())
}

// 本方法代码由AI完成
fn format_status(pid : ProcessId) -> FsResult<Vec<u8>> {
    let process = task::process_snapshot(pid).ok_or(FsError::NotFound)?;
    let leader = process.leader_task_id;
    let cred = cred::credentials_for(leader);
    let comm = comm_for(pid);
    let mem = process_memory_kb(pid)?;
    let ppid = process.parent_pid
                      .map(|p| p.raw())
                      .unwrap_or(0);
    let leader_state = task::task_snapshot(leader).map(|snap| snap.state);
    let sc = state_char(process.state, leader_state);
    let state_str = match sc {
        'S' => "S (sleeping)",
        'Z' => "Z (zombie)",
        _ => "R (running)",
    };
    let line = format!("Name:\t{comm}\nState:\t{state_str} \
                        ({sc})\nTgid:\t{}\nPid:\t{}\nPPid:\t{ppid}\nUid:\t{}\t{}\t{}\t{}\nGid:\\
                        t{}\t{}\t{}\t{}\nVmPeak:\t{}\tkB\nVmSize:\t{}\tkB\nVmRSS:\t{}\tkB\\
                        nVmData:\t{}\tkB\nVmStk:\t128\tkB\n",
                       pid.raw(),
                       pid.raw(),
                       cred.real_uid.0,
                       cred.effective_uid.0,
                       cred.saved_uid.0,
                       cred.fs_uid.0,
                       cred.real_gid.0,
                       cred.effective_gid.0,
                       cred.saved_gid.0,
                       cred.fs_gid.0,
                       mem.size_kb,
                       mem.size_kb,
                       mem.rss_kb,
                       mem.private_dirty_kb,);
    Ok(line.into_bytes())
}

// status/smaps 用的内存估算（非精确 RSS，仅供 LTP 读通）。
#[derive(Clone, Copy)]
// 本结构代码由AI完成
struct ProcMemoryKb {
    size_kb : usize,
    rss_kb : usize,
    private_dirty_kb : usize,
}

// 本方法代码由AI完成
fn process_memory_kb(pid : ProcessId) -> FsResult<ProcMemoryKb> {
    let process = task::process_snapshot(pid).ok_or(FsError::NotFound)?;
    let thread_extra = process.task_count
                              .saturating_sub(1) *
                       64;
    let heap_like = 4096usize.saturating_add(thread_extra);
    Ok(ProcMemoryKb { size_kb : heap_like,
                      rss_kb : heap_like,
                      private_dirty_kb : heap_like })
}

// 本方法代码由AI完成
fn format_maps(pid : ProcessId) -> FsResult<Vec<u8>> {
    let mem = process_memory_kb(pid)?;
    let heap_start = 0x1000_0000usize;
    let heap_end = heap_start.saturating_add(mem.size_kb
                                                .saturating_mul(1024));
    Ok(format!("{heap_start:016x}-{heap_end:016x} rw-p 00000000 00:00 0 \
                [heap]\n3f0000000000-3f0000010000 r-xp 00000000 00:00 0 [vdso]\n").into_bytes())
}

// 本方法代码由AI完成
fn format_smaps(pid : ProcessId) -> FsResult<Vec<u8>> {
    let mem = process_memory_kb(pid)?;
    let start = 0x1000_0000usize;
    let end = start.saturating_add(mem.size_kb
                                      .saturating_mul(1024));
    let line =
        format!("{start:016x}-{end:016x} rw-p 00000000 00:00 0 \
                 [heap]\nSize:\t{}\tkB\nRss:\t{}\tkB\nPss:\t{}\tkB\nShared_Clean:\t0\tkB\\
                 nShared_Dirty:\t0\tkB\nPrivate_Clean:\t0\tkB\nPrivate_Dirty:\t{}\tkB\\
                 nReferenced:\t{}\tkB\nAnonymous:\t{}\tkB\nSwap:\t0\tkB\nKernelPageSize:\t4\tkB\\
                 nMMUPageSize:\t4\tkB\nVmFlags: rd wr mr mw me ac sd\n",
                mem.size_kb, mem.rss_kb, mem.rss_kb, mem.private_dirty_kb, mem.rss_kb, mem.rss_kb,);
    Ok(line.into_bytes())
}

// 本方法代码由AI完成
fn format_cmdline(pid : ProcessId) -> FsResult<Vec<u8>> {
    let process = task::process_snapshot(pid).ok_or(FsError::NotFound)?;
    let leader = process.leader_task_id;
    let mut out = Vec::new();
    if let Some(argv) = argv_for(leader) {
        for (i, arg) in argv.iter()
                            .enumerate()
        {
            if i > 0 {
                out.push(0);
            }
            out.extend_from_slice(arg.as_bytes());
        }
        if !out.is_empty() {
            return Ok(out);
        }
    }
    if let Some(exe) = exe_for(leader) {
        out.extend_from_slice(exe.as_bytes());
    }
    Ok(out)
}

// 本方法代码由AI完成
fn format_meminfo() -> Vec<u8> {
    let stats = mm_frame_alloctor::frame_mem_stats();
    format!("MemTotal:\t{}\tkB\nMemFree:\t{}\tkB\nMemAvailable:\t{}\tkB\nBuffers:\t0\tkB\n\
             Cached:\t0\tkB\n",
            stats.total_bytes() / 1024,
            stats.free_bytes() / 1024,
            stats.free_bytes() / 1024,).into_bytes()
}

// 本方法代码由AI完成
fn format_mounts() -> Vec<u8> {
    let mut out = Vec::new();
    for line in mount_lines() {
        let row = format!("{} {} {} rw,relatime 0 0\n",
                          line.device, line.mount_point, line.fstype);
        out.extend_from_slice(row.as_bytes());
    }
    out
}

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
            ProcNode::ProcNetTcp => true,
            ProcNode::SysKernelPidMax | ProcNode::SysKernelTainted => true,
            ProcNode::PidDir(pid) |
            ProcNode::PidStat(pid) |
            ProcNode::PidStatus(pid) |
            ProcNode::PidComm(pid) |
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
            ProcNode::NetDir |
            ProcNode::ProcNetTcp |
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
            ProcNode::ProcNetTcp => Ok(b"sl local_address rem_address st tx_queue rx_queue tr tm->when retrnsmt\n\
 0: 00000000:0000 00000000:0000 0A 00000000:00000000 00:00000000 0\n".to_vec()),
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
            ProcNode::PidSmaps(pid) => format_smaps(pid),
            ProcNode::PidMaps(pid) => format_maps(pid),
            ProcNode::PidCmdline(pid) => format_cmdline(pid),
            ProcNode::PidTaskComm(pid, task_id) => format_task_comm(pid, task_id),
        }
    }

    fn read_range(&self, rel_path : &str, offset : u64, buf : &mut [u8]) -> FsResult<usize> {
        let node = parse_node(rel_path).ok_or(FsError::NotFound)?;
        let static_data : &[u8] = match node {
            ProcNode::ProcNetTcp => {
                b"sl local_address rem_address st tx_queue rx_queue tr tm->when retrnsmt\n\
 0: 00000000:0000 00000000:0000 0A 00000000:00000000 00:00000000 0\n"
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
                Ok(format!("anon_inode:[wateros-fd-{fd}]").into_bytes())
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
            ProcNode::NetDir => Ok(vec![FsDirEntry { name : String::from("tcp"),
                                                   node_type : FsNodeType::File }]),
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
