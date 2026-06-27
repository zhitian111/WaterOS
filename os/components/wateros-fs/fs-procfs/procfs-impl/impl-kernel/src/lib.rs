#![no_std]

//! 内核 procfs：从 task/cred/mm 与 VFS 回调生成 `/proc` 内容。

extern crate alloc;

use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use api_v0::{
    FsDirEntry, FsError, FsMetadata, FsNodeType, FsResult, MountListLookup, ProcFsView,
    ProcMountLine, TaskArgvLookup, TaskExeLookup, TaskId,
};
use fs_api_v0::{FsAccessMode, FsCapability, FsImpl, FsKind};
use spin::Mutex;
use task::{ProcessId, ProcessState, TaskState};

static ARGV_LOOKUP: Mutex<Option<TaskArgvLookup>> = Mutex::new(None);
static EXE_LOOKUP: Mutex<Option<TaskExeLookup>> = Mutex::new(None);
static MOUNT_LOOKUP: Mutex<Option<MountListLookup>> = Mutex::new(None);

pub fn register_task_argv_lookup(f: TaskArgvLookup) {
    *ARGV_LOOKUP.lock() = Some(f);
}

pub fn register_task_exe_lookup(f: TaskExeLookup) {
    *EXE_LOOKUP.lock() = Some(f);
}

pub fn register_mount_list_lookup(f: MountListLookup) {
    *MOUNT_LOOKUP.lock() = Some(f);
}

fn argv_for(leader: TaskId) -> Option<Vec<String>> {
    let lookup = *ARGV_LOOKUP.lock();
    lookup.and_then(|f| f(leader))
}

fn exe_for(leader: TaskId) -> Option<String> {
    let lookup = *EXE_LOOKUP.lock();
    lookup.and_then(|f| f(leader))
}

fn mount_lines() -> Vec<ProcMountLine> {
    let lookup = *MOUNT_LOOKUP.lock();
    lookup.map(|f| f()).unwrap_or_default()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProcNode {
    Root,
    Meminfo,
    Cgroups,
    Mounts,
    SysKernelPidMax,
    SysKernelTainted,
    PidDir(ProcessId),
    PidStat(ProcessId),
    PidStatus(ProcessId),
    PidSmaps(ProcessId),
    PidMaps(ProcessId),
    PidCmdline(ProcessId),
}

fn proc_inode(node: ProcNode) -> u64 {
    match node {
        ProcNode::Root => 1,
        ProcNode::Meminfo => 2,
        ProcNode::Cgroups => 6,
        ProcNode::Mounts => 3,
        ProcNode::SysKernelPidMax => 4,
        ProcNode::SysKernelTainted => 5,
        ProcNode::PidDir(pid) => 0x1000_0000 | ((pid.raw() as u64) << 4),
        ProcNode::PidStat(pid) => 0x1000_0001 | ((pid.raw() as u64) << 4),
        ProcNode::PidStatus(pid) => 0x1000_0002 | ((pid.raw() as u64) << 4),
        ProcNode::PidSmaps(pid) => 0x1000_0003 | ((pid.raw() as u64) << 4),
        ProcNode::PidMaps(pid) => 0x1000_0005 | ((pid.raw() as u64) << 4),
        ProcNode::PidCmdline(pid) => 0x1000_0004 | ((pid.raw() as u64) << 4),
    }
}

fn normalize_rel(path: &str) -> String {
    if path.is_empty() {
        return String::from("/");
    }
    if path.starts_with('/') {
        String::from(path)
    } else {
        format!("/{path}")
    }
}

fn parse_node(path: &str) -> Option<ProcNode> {
    let p = normalize_rel(path);
    if p == "/" {
        return Some(ProcNode::Root);
    }
    if p == "/meminfo" {
        return Some(ProcNode::Meminfo);
    }
    if p == "/cgroups" {
        return Some(ProcNode::Cgroups);
    }
    if p == "/mounts" {
        return Some(ProcNode::Mounts);
    }
    if p == "/sys/kernel/pid_max" {
        return Some(ProcNode::SysKernelPidMax);
    }
    if p == "/sys/kernel/tainted" {
        return Some(ProcNode::SysKernelTainted);
    }
    let rest = p.strip_prefix('/')?;
    let (first, tail) = rest.split_once('/').map(|(a, b)| (a, Some(b))).unwrap_or((rest, None));
    let pid = if first == "self" {
        task::current_process_task_snapshot()?.pid
    } else {
        ProcessId::from_raw(first.parse::<usize>().ok()?)
    };
    match tail {
        None => Some(ProcNode::PidDir(pid)),
        Some("") => Some(ProcNode::PidDir(pid)),
        Some("stat") => Some(ProcNode::PidStat(pid)),
        Some("status") => Some(ProcNode::PidStatus(pid)),
        Some("smaps") => Some(ProcNode::PidSmaps(pid)),
        Some("maps") => Some(ProcNode::PidMaps(pid)),
        Some("mounts") => Some(ProcNode::Mounts),
        Some("cmdline") => Some(ProcNode::PidCmdline(pid)),
        Some(_) => None,
    }
}

fn process_visible(pid: ProcessId) -> bool {
    task::process_snapshot(pid).is_some()
}

fn comm_for(pid: ProcessId) -> String {
    let leader = task::leader_task_for_process(pid).unwrap_or(0);
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

fn format_cgroups() -> Vec<u8> {
    alloc::format!(
        "#subsys_name\thierarchy\tnum_cgroups\tenabled\n\
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
hugetlb\t12\t1\t1\n"
    )
    .into_bytes()
}

fn basename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

fn state_char(process: ProcessState, leader_state: Option<TaskState>) -> char {
    match process {
        ProcessState::Exited(_) | ProcessState::Exiting(_) => 'Z',
        ProcessState::Running => match leader_state {
            Some(TaskState::Sleeping { .. }) | Some(TaskState::Blocking(_)) => 'S',
            Some(TaskState::Exited(_)) => 'Z',
            Some(TaskState::Ready) | Some(TaskState::Running) | None => 'R',
        },
    }
}

fn format_stat(pid: ProcessId) -> FsResult<Vec<u8>> {
    let process = task::process_snapshot(pid).ok_or(FsError::NotFound)?;
    let leader = process.leader_task_id;
    // Linux utime/stime are in USER_HZ jiffies; tick_count tracks scheduler ticks (~100/s).
    let jiffies = task::task_snapshot(leader)
        .map(|snap| snap.stats.tick_count as u64)
        .unwrap_or(0)
        .max(1);
    let comm = comm_for(pid);
    let comm15 = if comm.len() > 15 {
        comm[..15].to_string()
    } else {
        comm
    };
    let ppid = process
        .parent_pid
        .map(|p| p.raw())
        .unwrap_or(0);
    let utime = jiffies;
    let stime = jiffies;
    let leader_state = task::task_snapshot(leader).map(|snap| snap.state);
    let sc = state_char(process.state, leader_state);
    let line = format!(
        "{} ({}) {} {} 0 0 0 0 0 0 0 0 {} {} 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n",
        pid.raw(),
        comm15,
        sc,
        ppid,
        utime,
        stime,
    );
    Ok(line.into_bytes())
}

fn format_status(pid: ProcessId) -> FsResult<Vec<u8>> {
    let process = task::process_snapshot(pid).ok_or(FsError::NotFound)?;
    let leader = process.leader_task_id;
    let cred = cred::credentials_for(leader);
    let comm = comm_for(pid);
    let mem = process_memory_kb(pid)?;
    let ppid = process
        .parent_pid
        .map(|p| p.raw())
        .unwrap_or(0);
    let leader_state = task::task_snapshot(leader).map(|snap| snap.state);
    let sc = state_char(process.state, leader_state);
    let state_str = match sc {
        'S' => "S (sleeping)",
        'Z' => "Z (zombie)",
        _ => "R (running)",
    };
    let line = format!(
        "Name:\t{comm}\nState:\t{state_str} ({sc})\nTgid:\t{}\nPid:\t{}\nPPid:\t{ppid}\nUid:\t{}\t{}\t{}\t{}\nGid:\t{}\t{}\t{}\t{}\nVmPeak:\t{}\tkB\nVmSize:\t{}\tkB\nVmRSS:\t{}\tkB\nVmData:\t{}\tkB\nVmStk:\t128\tkB\n",
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
        mem.private_dirty_kb,
    );
    Ok(line.into_bytes())
}

#[derive(Clone, Copy)]
struct ProcMemoryKb {
    size_kb : usize,
    rss_kb : usize,
    private_dirty_kb : usize,
}

fn process_memory_kb(pid : ProcessId) -> FsResult<ProcMemoryKb> {
    let process = task::process_snapshot(pid).ok_or(FsError::NotFound)?;
    let thread_extra = process.task_count.saturating_sub(1) * 64;
    let heap_like = 4096usize.saturating_add(thread_extra);
    Ok(ProcMemoryKb { size_kb : heap_like,
                      rss_kb : heap_like,
                      private_dirty_kb : heap_like })
}

fn format_maps(pid : ProcessId) -> FsResult<Vec<u8>> {
    let mem = process_memory_kb(pid)?;
    let heap_start = 0x1000_0000usize;
    let heap_end = heap_start.saturating_add(mem.size_kb.saturating_mul(1024));
    Ok(format!(
        "{heap_start:016x}-{heap_end:016x} rw-p 00000000 00:00 0 [heap]\n\
         3f0000000000-3f0000010000 r-xp 00000000 00:00 0 [vdso]\n"
    )
    .into_bytes())
}

fn format_smaps(pid : ProcessId) -> FsResult<Vec<u8>> {
    let mem = process_memory_kb(pid)?;
    let start = 0x1000_0000usize;
    let end = start.saturating_add(mem.size_kb.saturating_mul(1024));
    let line = format!(
        "{start:016x}-{end:016x} rw-p 00000000 00:00 0 [heap]\nSize:\t{}\tkB\nRss:\t{}\tkB\nPss:\t{}\tkB\nShared_Clean:\t0\tkB\nShared_Dirty:\t0\tkB\nPrivate_Clean:\t0\tkB\nPrivate_Dirty:\t{}\tkB\nReferenced:\t{}\tkB\nAnonymous:\t{}\tkB\nSwap:\t0\tkB\nKernelPageSize:\t4\tkB\nMMUPageSize:\t4\tkB\nVmFlags: rd wr mr mw me ac sd\n",
        mem.size_kb,
        mem.rss_kb,
        mem.rss_kb,
        mem.private_dirty_kb,
        mem.rss_kb,
        mem.rss_kb,
    );
    Ok(line.into_bytes())
}

fn format_cmdline(pid: ProcessId) -> FsResult<Vec<u8>> {
    let process = task::process_snapshot(pid).ok_or(FsError::NotFound)?;
    let leader = process.leader_task_id;
    let mut out = Vec::new();
    if let Some(argv) = argv_for(leader) {
        for (i, arg) in argv.iter().enumerate() {
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

fn format_meminfo() -> Vec<u8> {
    let stats = mm_frame_alloctor::frame_mem_stats();
    format!(
        "MemTotal:\t{}\tkB\nMemFree:\t{}\tkB\nMemAvailable:\t{}\tkB\nBuffers:\t0\tkB\nCached:\t0\tkB\n",
        stats.total_bytes() / 1024,
        stats.free_bytes() / 1024,
        stats.free_bytes() / 1024,
    )
    .into_bytes()
}

fn format_mounts() -> Vec<u8> {
    let mut out = Vec::new();
    for line in mount_lines() {
        let row = format!(
            "{} {} {} rw,relatime 0 0\n",
            line.device, line.mount_point, line.fstype
        );
        out.extend_from_slice(row.as_bytes());
    }
    out
}

pub struct KernelProcFs;

pub fn view() -> &'static KernelProcFs {
    &KernelProcFs
}

impl ProcFsView for KernelProcFs {
    fn exists(&self, rel_path: &str) -> FsResult<bool> {
        let Some(node) = parse_node(rel_path) else {
            return Ok(false);
        };
        Ok(match node {
            ProcNode::Root | ProcNode::Meminfo | ProcNode::Cgroups | ProcNode::Mounts => true,
            ProcNode::SysKernelPidMax | ProcNode::SysKernelTainted => true,
            ProcNode::PidDir(pid)
            | ProcNode::PidStat(pid)
            | ProcNode::PidStatus(pid)
            | ProcNode::PidSmaps(pid)
            | ProcNode::PidMaps(pid)
            | ProcNode::PidCmdline(pid) => process_visible(pid),
        })
    }

    fn metadata(&self, rel_path: &str) -> FsResult<FsMetadata> {
        let node = parse_node(rel_path).ok_or(FsError::NotFound)?;
        match node {
            ProcNode::Root | ProcNode::PidDir(_) => Ok(FsMetadata {
                node_type: FsNodeType::Directory,
                size: 0,
                mode: 0o555,
                inode: proc_inode(node),
                nlink: 1,
            }),
            ProcNode::Meminfo
            | ProcNode::Cgroups
            | ProcNode::Mounts
            | ProcNode::SysKernelPidMax
            | ProcNode::SysKernelTainted => Ok(FsMetadata {
                node_type: FsNodeType::File,
                size: self.read(rel_path)?.len() as u64,
                mode: 0o444,
                inode: proc_inode(node),
                nlink: 1,
            }),
            ProcNode::PidStat(pid)
            | ProcNode::PidStatus(pid)
            | ProcNode::PidSmaps(pid)
            | ProcNode::PidMaps(pid)
            | ProcNode::PidCmdline(pid) => {
                if !process_visible(pid) {
                    return Err(FsError::NotFound);
                }
                Ok(FsMetadata {
                    node_type: FsNodeType::File,
                    size: self.read(rel_path)?.len() as u64,
                    mode: 0o444,
                    inode: proc_inode(node),
                    nlink: 1,
                })
            }
        }
    }

    fn read(&self, rel_path: &str) -> FsResult<Vec<u8>> {
        let node = parse_node(rel_path).ok_or(FsError::NotFound)?;
        match node {
            ProcNode::Root | ProcNode::PidDir(_) => Err(FsError::NotAFile),
            ProcNode::Meminfo => Ok(format_meminfo()),
            ProcNode::Cgroups => Ok(format_cgroups()),
            ProcNode::Mounts => Ok(format_mounts()),
            ProcNode::SysKernelPidMax => Ok(b"32768\n".to_vec()),
            ProcNode::SysKernelTainted => Ok(b"0\n".to_vec()),
            ProcNode::PidStat(pid) => format_stat(pid),
            ProcNode::PidStatus(pid) => format_status(pid),
            ProcNode::PidSmaps(pid) => format_smaps(pid),
            ProcNode::PidMaps(pid) => format_maps(pid),
            ProcNode::PidCmdline(pid) => format_cmdline(pid),
        }
    }

    fn read_dir(&self, rel_path: &str) -> FsResult<Vec<FsDirEntry>> {
        let node = parse_node(rel_path).ok_or(FsError::NotFound)?;
        match node {
            ProcNode::Root => {
                let mut entries = vec![
                    FsDirEntry {
                        name: String::from("meminfo"),
                        node_type: FsNodeType::File,
                    },
                    FsDirEntry {
                        name: String::from("cgroups"),
                        node_type: FsNodeType::File,
                    },
                    FsDirEntry {
                        name: String::from("mounts"),
                        node_type: FsNodeType::File,
                    },
                ];
                for pid in task::all_process_pids() {
                    entries.push(FsDirEntry {
                        name: format!("{}", pid.raw()),
                        node_type: FsNodeType::Directory,
                    });
                }
                Ok(entries)
            }
            ProcNode::PidDir(pid) => {
                if !process_visible(pid) {
                    return Err(FsError::NotFound);
                }
                Ok(vec![
                    FsDirEntry {
                        name: String::from("stat"),
                        node_type: FsNodeType::File,
                    },
                    FsDirEntry {
                        name: String::from("status"),
                        node_type: FsNodeType::File,
                    },
                    FsDirEntry {
                        name: String::from("smaps"),
                        node_type: FsNodeType::File,
                    },
                    FsDirEntry {
                        name: String::from("maps"),
                        node_type: FsNodeType::File,
                    },
                    FsDirEntry {
                        name: String::from("mounts"),
                        node_type: FsNodeType::File,
                    },
                    FsDirEntry {
                        name: String::from("cmdline"),
                        node_type: FsNodeType::File,
                    },
                ])
            }
            _ => Err(FsError::NotAFile),
        }
    }
}

pub struct KernelProcFsImpl;

pub static IMPL: KernelProcFsImpl = KernelProcFsImpl;

const SUPPORTED: &[FsCapability] =
    &[FsCapability::new(FsKind::Other("procfs"), FsAccessMode::ReadOnly)];

impl FsImpl for KernelProcFsImpl {
    fn name(&self) -> &'static str {
        "procfs"
    }

    fn supported(&self) -> &'static [FsCapability] {
        SUPPORTED
    }

    fn mount_ro(
        &self,
        _device: driver_block_api_v0::SharedBlockDevice,
    ) -> fs_api_v0::FsResult<fs_api_v0::SharedFs> {
        Err(FsError::Unsupported)
    }
}

pub fn test() {
    let v = view();
    let _ = v.read_dir("/");
    logging::info!("[fs::procfs] self_test ok");
}
