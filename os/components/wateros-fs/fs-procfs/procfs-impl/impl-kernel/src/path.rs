use super::*;

// 内部路径解析结果；覆盖全局文件与 per-pid 子树。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProcNode {
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
    SysKernelCapLastCap,
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
pub(crate) fn proc_inode(node : ProcNode) -> u64 {
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
        ProcNode::SysKernelCapLastCap => 19,
        ProcNode::PidDir(pid) => 0x1000_0000 | ((pid.raw() as u64) << 4),
        ProcNode::PidStat(pid) => 0x1000_0001 | ((pid.raw() as u64) << 4),
        ProcNode::PidStatus(pid) => 0x1000_0002 | ((pid.raw() as u64) << 4),
        ProcNode::PidComm(pid) => 0x1000_0009 | ((pid.raw() as u64) << 4),
        ProcNode::PidTimerSlack(pid) => 0x1000_000A | ((pid.raw() as u64) << 4),
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
        ProcNode::PidFd(pid, fd) => 0x2000_0000_0000_0000 | ((pid.raw() as u64) << 32) | fd as u64,
    }
}

/// 与 VFS `normalize_absolute_path` 一致：折叠 `//`、`.`，解析 `..`。
// 本方法代码由AI完成
pub(crate) fn normalize_rel(path : &str) -> String {
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
pub(crate) fn parse_pid(name : &str) -> Option<ProcessId> {
    if name == "self" {
        Some(task::current_process_task_snapshot()?.pid)
    } else {
        Some(ProcessId::from_raw(name.parse::<usize>()
                                     .ok()?))
    }
}

pub(crate) fn parse_thread_task(pid : ProcessId, name : &str) -> Option<TaskId> {
    let tid = name.parse::<usize>()
                  .ok()?;
    let task_id = task::task_id_for_thread(ThreadId::from_raw(tid))?;
    task::task_ids_for_process(pid)?.contains(&task_id)
                                    .then_some(task_id)
}

// 将相对 `/proc` 的路径映射为内部节点；未知路径返回 None。
// 本方法代码由AI完成
pub(crate) fn parse_node(path : &str) -> Option<ProcNode> {
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
        ["sys", "kernel", "cap_last_cap"] => Some(ProcNode::SysKernelCapLastCap),
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
        [pid_name, "fd", fd] => Some(ProcNode::PidFd(parse_pid(pid_name)?, fd.parse().ok()?)),
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
