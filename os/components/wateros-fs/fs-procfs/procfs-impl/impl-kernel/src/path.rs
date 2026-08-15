use super::*;

/// WaterOS 首期只有一组全局 namespace；这些类型用于向 procfs 发布稳定身份，
/// 不表示已经支持 `clone(CLONE_NEW*)` 或 `unshare()`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProcNamespace {
    Cgroup,
    Ipc,
    Mnt,
    Net,
    Pid,
    PidForChildren,
    Time,
    TimeForChildren,
    User,
    Uts,
}

impl ProcNamespace {
    pub(crate) const ALL : [Self; 10] = [Self::Cgroup,
                                         Self::Ipc,
                                         Self::Mnt,
                                         Self::Net,
                                         Self::Pid,
                                         Self::PidForChildren,
                                         Self::Time,
                                         Self::TimeForChildren,
                                         Self::User,
                                         Self::Uts];

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Cgroup => "cgroup",
            Self::Ipc => "ipc",
            Self::Mnt => "mnt",
            Self::Net => "net",
            Self::Pid => "pid",
            Self::PidForChildren => "pid_for_children",
            Self::Time => "time",
            Self::TimeForChildren => "time_for_children",
            Self::User => "user",
            Self::Uts => "uts",
        }
    }

    pub(crate) const fn inode(self) -> u64 {
        // 只要求同类 namespace 在所有进程间身份一致；采用 Linux 初始
        // namespace 常见编号，便于工具输出和人工识别。
        match self {
            Self::Mnt => 4_026_531_840,
            Self::Uts => 4_026_531_838,
            Self::Ipc => 4_026_531_839,
            Self::User => 4_026_531_837,
            Self::Pid | Self::PidForChildren => 4_026_531_836,
            Self::Net => 4_026_531_992,
            Self::Cgroup => 4_026_531_835,
            Self::Time | Self::TimeForChildren => 4_026_531_834,
        }
    }

    pub(crate) fn parse(name : &str) -> Option<Self> {
        Self::ALL.into_iter().find(|namespace| namespace.name() == name)
    }
}

// 内部路径解析结果；覆盖全局文件与 per-pid 子树。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProcNode {
    Root,
    Meminfo,
    Cpuinfo,
    Stat,
    Loadavg,
    Version,
    Filesystems,
    Devices,
    Swaps,
    Partitions,
    Interrupts,
    Cmdline,
    Vmstat,
    Diskstats,
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
    ProcNetDev,
    ProcNetRoute,
    ProcNetSockstat,
    ProcNetSockstat6,
    PressureDir,
    PressureCpu,
    PressureIo,
    PressureMemory,
    SysVIpcDir,
    SysVIpcShm,
    SysVIpcMsg,
    SysVIpcSem,
    SysDir,
    SysKernelDir,
    SysVmDir,
    SysFsDir,
    SysKernelPidMax,
    SysKernelTainted,
    SysKernelCapLastCap,
    SysKernelOsType,
    SysKernelOsRelease,
    SysKernelVersion,
    SysKernelHostname,
    SysKernelDomainname,
    SysKernelThreadsMax,
    SysKernelNgroupsMax,
    SysKernelShmMax,
    SysKernelShmAll,
    SysKernelShmMni,
    SysKernelShmRmidForced,
    SysVmOvercommitMemory,
    SysVmMaxMapCount,
    SysVmMmapMinAddr,
    SysFsFileMax,
    SysFsNrOpen,
    SysFsPipeMaxSize,
    SysFsFileNr,
    SysFsAioMaxNr,
    SysNetDir,
    SysNetCoreDir,
    SysNetIpv4Dir,
    SysNetCoreSomaxconn,
    SysNetIpv4PortRange,
    SysNetIpv4TcpSyncookies,
    SysKernelRandomDir,
    SysKernelRandomBootId,
    SysKernelRandomUuid,
    SysKernelRandomizeVaSpace,
    SelfLink,
    ThreadSelfLink,
    PidDir(ProcessId),
    PidStat(ProcessId),
    PidStatus(ProcessId),
    PidComm(ProcessId),
    PidTimerSlack(ProcessId),
    PidSmaps(ProcessId),
    PidMaps(ProcessId),
    PidCmdline(ProcessId),
    PidStatm(ProcessId),
    PidLimits(ProcessId),
    PidMounts(ProcessId),
    PidMountinfo(ProcessId),
    PidCgroup(ProcessId),
    PidWchan(ProcessId),
    PidExe(ProcessId),
    PidCwd(ProcessId),
    PidRoot(ProcessId),
    PidFdDir(ProcessId),
    PidFd(ProcessId, usize),
    PidFdInfoDir(ProcessId),
    PidFdInfo(ProcessId, usize),
    PidNsDir(ProcessId),
    PidNamespace(ProcessId, ProcNamespace),
    PidTaskRoot(ProcessId),
    PidTaskDir(ProcessId, TaskId),
    PidTaskComm(ProcessId, TaskId),
    PidTaskStat(ProcessId, TaskId),
    PidTaskStatus(ProcessId, TaskId),
    PidTaskWchan(ProcessId, TaskId),
}

// 为 proc 节点分配稳定 inode 号（pid 子树按 pid 编码）。
// 本方法代码由AI完成
pub(crate) fn proc_inode(node : ProcNode) -> u64 {
    match node {
        ProcNode::Root => 1,
        ProcNode::Meminfo => 2,
        ProcNode::Cpuinfo => 7,
        ProcNode::Stat => 20,
        ProcNode::Loadavg => 21,
        ProcNode::Version => 22,
        ProcNode::Filesystems => 23,
        ProcNode::Devices => 24,
        ProcNode::Swaps => 25,
        ProcNode::Partitions => 26,
        ProcNode::Interrupts => 27,
        ProcNode::Cmdline => 53,
        ProcNode::Vmstat => 54,
        ProcNode::Diskstats => 55,
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
        ProcNode::ProcNetDev => 56,
        ProcNode::ProcNetRoute => 57,
        ProcNode::ProcNetSockstat => 58,
        ProcNode::ProcNetSockstat6 => 59,
        ProcNode::PressureDir => 60,
        ProcNode::PressureCpu => 61,
        ProcNode::PressureIo => 62,
        ProcNode::PressureMemory => 63,
        ProcNode::SysVIpcDir => 45,
        ProcNode::SysVIpcShm => 46,
        ProcNode::SysVIpcMsg => 47,
        ProcNode::SysVIpcSem => 48,
        ProcNode::SysDir => 9,
        ProcNode::SysKernelDir => 10,
        ProcNode::SysVmDir => 28,
        ProcNode::SysFsDir => 29,
        ProcNode::SysKernelPidMax => 4,
        ProcNode::SysKernelTainted => 5,
        ProcNode::SysKernelCapLastCap => 19,
        ProcNode::SysKernelOsType => 30,
        ProcNode::SysKernelOsRelease => 31,
        ProcNode::SysKernelVersion => 32,
        ProcNode::SysKernelHostname => 33,
        ProcNode::SysKernelDomainname => 34,
        ProcNode::SysKernelThreadsMax => 35,
        ProcNode::SysKernelNgroupsMax => 36,
        ProcNode::SysKernelShmMax => 49,
        ProcNode::SysKernelShmAll => 50,
        ProcNode::SysKernelShmMni => 51,
        ProcNode::SysKernelShmRmidForced => 52,
        ProcNode::SysVmOvercommitMemory => 37,
        ProcNode::SysVmMaxMapCount => 38,
        ProcNode::SysVmMmapMinAddr => 39,
        ProcNode::SysFsFileMax => 40,
        ProcNode::SysFsNrOpen => 41,
        ProcNode::SysFsPipeMaxSize => 42,
        ProcNode::SysFsFileNr => 64,
        ProcNode::SysFsAioMaxNr => 65,
        ProcNode::SysNetDir => 66,
        ProcNode::SysNetCoreDir => 67,
        ProcNode::SysNetIpv4Dir => 68,
        ProcNode::SysNetCoreSomaxconn => 69,
        ProcNode::SysNetIpv4PortRange => 70,
        ProcNode::SysNetIpv4TcpSyncookies => 71,
        ProcNode::SysKernelRandomDir => 72,
        ProcNode::SysKernelRandomBootId => 73,
        ProcNode::SysKernelRandomUuid => 74,
        ProcNode::SysKernelRandomizeVaSpace => 75,
        ProcNode::SelfLink => 43,
        ProcNode::ThreadSelfLink => 44,
        ProcNode::PidDir(pid) => 0x1000_0000 | ((pid.raw() as u64) << 4),
        ProcNode::PidStat(pid) => 0x1000_0001 | ((pid.raw() as u64) << 4),
        ProcNode::PidStatus(pid) => 0x1000_0002 | ((pid.raw() as u64) << 4),
        ProcNode::PidComm(pid) => 0x1000_0009 | ((pid.raw() as u64) << 4),
        ProcNode::PidTimerSlack(pid) => 0x1000_000A | ((pid.raw() as u64) << 4),
        ProcNode::PidSmaps(pid) => 0x1000_0003 | ((pid.raw() as u64) << 4),
        ProcNode::PidMaps(pid) => 0x1000_0005 | ((pid.raw() as u64) << 4),
        ProcNode::PidCmdline(pid) => 0x1000_0004 | ((pid.raw() as u64) << 4),
        ProcNode::PidStatm(pid) => 0x4000_0001 | ((pid.raw() as u64) << 8),
        ProcNode::PidLimits(pid) => 0x4000_0002 | ((pid.raw() as u64) << 8),
        ProcNode::PidMountinfo(pid) => 0x4000_0003 | ((pid.raw() as u64) << 8),
        ProcNode::PidWchan(pid) => 0x4000_0004 | ((pid.raw() as u64) << 8),
        ProcNode::PidExe(pid) => 0x1000_0006 | ((pid.raw() as u64) << 4),
        ProcNode::PidCwd(pid) => 0x4000_0005 | ((pid.raw() as u64) << 8),
        ProcNode::PidRoot(pid) => 0x4000_0006 | ((pid.raw() as u64) << 8),
        ProcNode::PidFdDir(pid) => 0x1000_0007 | ((pid.raw() as u64) << 4),
        ProcNode::PidFdInfoDir(pid) => 0x4000_0007 | ((pid.raw() as u64) << 8),
        ProcNode::PidNsDir(pid) => 0x4000_0008 | ((pid.raw() as u64) << 8),
        ProcNode::PidMounts(pid) => 0x4000_0009 | ((pid.raw() as u64) << 8),
        ProcNode::PidCgroup(pid) => 0x4000_000A | ((pid.raw() as u64) << 8),
        ProcNode::PidNamespace(_, namespace) => namespace.inode(),
        ProcNode::PidTaskRoot(pid) => 0x1000_0008 | ((pid.raw() as u64) << 4),
        ProcNode::PidTaskDir(pid, tid) => {
            0x3000_0000_0000_0000 | ((pid.raw() as u64) << 32) | (tid as u64)
        }
        ProcNode::PidTaskComm(pid, tid) => {
            0x3100_0000_0000_0000 | ((pid.raw() as u64) << 32) | (tid as u64)
        }
        ProcNode::PidTaskStat(pid, tid) => {
            0x3200_0000_0000_0000 | ((pid.raw() as u64) << 32) | (tid as u64)
        }
        ProcNode::PidTaskStatus(pid, tid) => {
            0x3300_0000_0000_0000 | ((pid.raw() as u64) << 32) | (tid as u64)
        }
        ProcNode::PidTaskWchan(pid, tid) => {
            0x3400_0000_0000_0000 | ((pid.raw() as u64) << 32) | (tid as u64)
        }
        ProcNode::PidFd(pid, fd) => 0x2000_0000_0000_0000 | ((pid.raw() as u64) << 32) | fd as u64,
        ProcNode::PidFdInfo(pid, fd) => 0x2100_0000_0000_0000 | ((pid.raw() as u64) << 32) | fd as u64,
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
        ["stat"] => Some(ProcNode::Stat),
        ["loadavg"] => Some(ProcNode::Loadavg),
        ["version"] => Some(ProcNode::Version),
        ["filesystems"] => Some(ProcNode::Filesystems),
        ["devices"] => Some(ProcNode::Devices),
        ["swaps"] => Some(ProcNode::Swaps),
        ["partitions"] => Some(ProcNode::Partitions),
        ["interrupts"] => Some(ProcNode::Interrupts),
        ["cmdline"] => Some(ProcNode::Cmdline),
        ["vmstat"] => Some(ProcNode::Vmstat),
        ["diskstats"] => Some(ProcNode::Diskstats),
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
        ["net", "dev"] => Some(ProcNode::ProcNetDev),
        ["net", "route"] => Some(ProcNode::ProcNetRoute),
        ["net", "sockstat"] => Some(ProcNode::ProcNetSockstat),
        ["net", "sockstat6"] => Some(ProcNode::ProcNetSockstat6),
        ["pressure"] => Some(ProcNode::PressureDir),
        ["pressure", "cpu"] => Some(ProcNode::PressureCpu),
        ["pressure", "io"] => Some(ProcNode::PressureIo),
        ["pressure", "memory"] => Some(ProcNode::PressureMemory),
        ["sysvipc"] => Some(ProcNode::SysVIpcDir),
        ["sysvipc", "shm"] => Some(ProcNode::SysVIpcShm),
        ["sysvipc", "msg"] => Some(ProcNode::SysVIpcMsg),
        ["sysvipc", "sem"] => Some(ProcNode::SysVIpcSem),
        ["sys"] => Some(ProcNode::SysDir),
        ["sys", "kernel"] => Some(ProcNode::SysKernelDir),
        ["sys", "kernel", "random"] => Some(ProcNode::SysKernelRandomDir),
        ["sys", "vm"] => Some(ProcNode::SysVmDir),
        ["sys", "fs"] => Some(ProcNode::SysFsDir),
        ["sys", "net"] => Some(ProcNode::SysNetDir),
        ["sys", "net", "core"] => Some(ProcNode::SysNetCoreDir),
        ["sys", "net", "ipv4"] => Some(ProcNode::SysNetIpv4Dir),
        ["sys", "kernel", "pid_max"] => Some(ProcNode::SysKernelPidMax),
        ["sys", "kernel", "tainted"] => Some(ProcNode::SysKernelTainted),
        ["sys", "kernel", "cap_last_cap"] => Some(ProcNode::SysKernelCapLastCap),
        ["sys", "kernel", "ostype"] => Some(ProcNode::SysKernelOsType),
        ["sys", "kernel", "osrelease"] => Some(ProcNode::SysKernelOsRelease),
        ["sys", "kernel", "version"] => Some(ProcNode::SysKernelVersion),
        ["sys", "kernel", "hostname"] => Some(ProcNode::SysKernelHostname),
        ["sys", "kernel", "domainname"] => Some(ProcNode::SysKernelDomainname),
        ["sys", "kernel", "threads-max"] => Some(ProcNode::SysKernelThreadsMax),
        ["sys", "kernel", "ngroups_max"] => Some(ProcNode::SysKernelNgroupsMax),
        ["sys", "kernel", "shmmax"] => Some(ProcNode::SysKernelShmMax),
        ["sys", "kernel", "shmall"] => Some(ProcNode::SysKernelShmAll),
        ["sys", "kernel", "shmmni"] => Some(ProcNode::SysKernelShmMni),
        ["sys", "kernel", "shm_rmid_forced"] => Some(ProcNode::SysKernelShmRmidForced),
        ["sys", "kernel", "random", "boot_id"] => Some(ProcNode::SysKernelRandomBootId),
        ["sys", "kernel", "random", "uuid"] => Some(ProcNode::SysKernelRandomUuid),
        ["sys", "kernel", "randomize_va_space"] => Some(ProcNode::SysKernelRandomizeVaSpace),
        ["sys", "vm", "overcommit_memory"] => Some(ProcNode::SysVmOvercommitMemory),
        ["sys", "vm", "max_map_count"] => Some(ProcNode::SysVmMaxMapCount),
        ["sys", "vm", "mmap_min_addr"] => Some(ProcNode::SysVmMmapMinAddr),
        ["sys", "fs", "file-max"] => Some(ProcNode::SysFsFileMax),
        ["sys", "fs", "nr_open"] => Some(ProcNode::SysFsNrOpen),
        ["sys", "fs", "pipe-max-size"] => Some(ProcNode::SysFsPipeMaxSize),
        ["sys", "fs", "file-nr"] => Some(ProcNode::SysFsFileNr),
        ["sys", "fs", "aio-max-nr"] => Some(ProcNode::SysFsAioMaxNr),
        ["sys", "net", "core", "somaxconn"] => Some(ProcNode::SysNetCoreSomaxconn),
        ["sys", "net", "ipv4", "ip_local_port_range"] => Some(ProcNode::SysNetIpv4PortRange),
        ["sys", "net", "ipv4", "tcp_syncookies"] => Some(ProcNode::SysNetIpv4TcpSyncookies),
        ["self"] => Some(ProcNode::SelfLink),
        ["thread-self"] => Some(ProcNode::ThreadSelfLink),
        [pid_name] => Some(ProcNode::PidDir(parse_pid(pid_name)?)),
        [pid_name, "stat"] => Some(ProcNode::PidStat(parse_pid(pid_name)?)),
        [pid_name, "status"] => Some(ProcNode::PidStatus(parse_pid(pid_name)?)),
        [pid_name, "comm"] => Some(ProcNode::PidComm(parse_pid(pid_name)?)),
        [pid_name, "timerslack_ns"] => Some(ProcNode::PidTimerSlack(parse_pid(pid_name)?)),
        [pid_name, "smaps"] => Some(ProcNode::PidSmaps(parse_pid(pid_name)?)),
        [pid_name, "maps"] => Some(ProcNode::PidMaps(parse_pid(pid_name)?)),
        [pid_name, "mounts"] => Some(ProcNode::PidMounts(parse_pid(pid_name)?)),
        [pid_name, "cmdline"] => Some(ProcNode::PidCmdline(parse_pid(pid_name)?)),
        [pid_name, "statm"] => Some(ProcNode::PidStatm(parse_pid(pid_name)?)),
        [pid_name, "limits"] => Some(ProcNode::PidLimits(parse_pid(pid_name)?)),
        [pid_name, "mountinfo"] => Some(ProcNode::PidMountinfo(parse_pid(pid_name)?)),
        [pid_name, "cgroup"] => Some(ProcNode::PidCgroup(parse_pid(pid_name)?)),
        [pid_name, "wchan"] => Some(ProcNode::PidWchan(parse_pid(pid_name)?)),
        [pid_name, "exe"] => Some(ProcNode::PidExe(parse_pid(pid_name)?)),
        [pid_name, "cwd"] => Some(ProcNode::PidCwd(parse_pid(pid_name)?)),
        [pid_name, "root"] => Some(ProcNode::PidRoot(parse_pid(pid_name)?)),
        [pid_name, "fd"] => Some(ProcNode::PidFdDir(parse_pid(pid_name)?)),
        [pid_name, "fdinfo"] => Some(ProcNode::PidFdInfoDir(parse_pid(pid_name)?)),
        [pid_name, "ns"] => Some(ProcNode::PidNsDir(parse_pid(pid_name)?)),
        [pid_name, "ns", namespace] => {
            Some(ProcNode::PidNamespace(parse_pid(pid_name)?, ProcNamespace::parse(namespace)?))
        }
        [pid_name, "task"] => Some(ProcNode::PidTaskRoot(parse_pid(pid_name)?)),
        [pid_name, "fd", fd] => Some(ProcNode::PidFd(parse_pid(pid_name)?, fd.parse().ok()?)),
        [pid_name, "fdinfo", fd] => Some(ProcNode::PidFdInfo(parse_pid(pid_name)?, fd.parse().ok()?)),
        [pid_name, "task", tid_name] => {
            let pid = parse_pid(pid_name)?;
            Some(ProcNode::PidTaskDir(pid, parse_thread_task(pid, tid_name)?))
        }
        [pid_name, "task", tid_name, "comm"] => {
            let pid = parse_pid(pid_name)?;
            Some(ProcNode::PidTaskComm(pid, parse_thread_task(pid, tid_name)?))
        }
        [pid_name, "task", tid_name, "stat"] => {
            let pid = parse_pid(pid_name)?;
            Some(ProcNode::PidTaskStat(pid, parse_thread_task(pid, tid_name)?))
        }
        [pid_name, "task", tid_name, "status"] => {
            let pid = parse_pid(pid_name)?;
            Some(ProcNode::PidTaskStatus(pid, parse_thread_task(pid, tid_name)?))
        }
        [pid_name, "task", tid_name, "wchan"] => {
            let pid = parse_pid(pid_name)?;
            Some(ProcNode::PidTaskWchan(pid, parse_thread_task(pid, tid_name)?))
        }
        _ => None,
    }
}
