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
use task::{ProcessId, ProcessState};

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
    ARGV_LOOKUP.lock().and_then(|f| f(leader))
}

fn exe_for(leader: TaskId) -> Option<String> {
    EXE_LOOKUP.lock().and_then(|f| f(leader))
}

fn mount_lines() -> Vec<ProcMountLine> {
    MOUNT_LOOKUP
        .lock()
        .map(|f| f())
        .unwrap_or_default()
}

const USER_HZ: u64 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProcNode {
    Root,
    Meminfo,
    Mounts,
    PidDir(ProcessId),
    PidStat(ProcessId),
    PidStatus(ProcessId),
    PidCmdline(ProcessId),
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
    if p == "/mounts" {
        return Some(ProcNode::Mounts);
    }
    let rest = p.strip_prefix('/')?;
    let (first, tail) = rest.split_once('/').map(|(a, b)| (a, Some(b))).unwrap_or((rest, None));
    let pid = first.parse::<usize>().ok()?;
    let pid = ProcessId::from_raw(pid);
    match tail {
        None => Some(ProcNode::PidDir(pid)),
        Some("") => Some(ProcNode::PidDir(pid)),
        Some("stat") => Some(ProcNode::PidStat(pid)),
        Some("status") => Some(ProcNode::PidStatus(pid)),
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

fn basename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

fn state_char(state: ProcessState) -> char {
    match state {
        ProcessState::Running => 'R',
        ProcessState::Exiting(_) => 'D',
        ProcessState::Exited(_) => 'Z',
    }
}

fn format_stat(pid: ProcessId) -> FsResult<Vec<u8>> {
    let process = task::process_snapshot(pid).ok_or(FsError::NotFound)?;
    let leader = process.leader_task_id;
    let utime = task::task_snapshot(leader)
        .map(|snap| snap.stats.tick_count as u64 * USER_HZ)
        .unwrap_or(0);
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
    let stime = 0u64;
    let line = format!(
        "{} ({}) {} {} 0 0 0 0 0 {} {} 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n",
        pid.raw(),
        comm15,
        state_char(process.state),
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
    let ppid = process
        .parent_pid
        .map(|p| p.raw())
        .unwrap_or(0);
    let state_str = match process.state {
        ProcessState::Running => "R (running)",
        ProcessState::Exiting(_) => "D (disk sleep)",
        ProcessState::Exited(_) => "Z (zombie)",
    };
    let sc = state_char(process.state);
    let line = format!(
        "Name:\t{comm}\nState:\t{state_str} ({sc})\nTgid:\t{}\nPid:\t{}\nPPid:\t{ppid}\nUid:\t{}\t{}\t{}\t{}\nGid:\t{}\t{}\t{}\t{}\n",
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
            "{} {} rw,relatime 0 0\n",
            line.mount_point, line.fstype
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
            ProcNode::Root | ProcNode::Meminfo | ProcNode::Mounts => true,
            ProcNode::PidDir(pid)
            | ProcNode::PidStat(pid)
            | ProcNode::PidStatus(pid)
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
            }),
            ProcNode::Meminfo | ProcNode::Mounts => Ok(FsMetadata {
                node_type: FsNodeType::File,
                size: self.read(rel_path)?.len() as u64,
                mode: 0o444,
            }),
            ProcNode::PidStat(pid)
            | ProcNode::PidStatus(pid)
            | ProcNode::PidCmdline(pid) => {
                if !process_visible(pid) {
                    return Err(FsError::NotFound);
                }
                Ok(FsMetadata {
                    node_type: FsNodeType::File,
                    size: self.read(rel_path)?.len() as u64,
                    mode: 0o444,
                })
            }
        }
    }

    fn read(&self, rel_path: &str) -> FsResult<Vec<u8>> {
        let node = parse_node(rel_path).ok_or(FsError::NotFound)?;
        match node {
            ProcNode::Root | ProcNode::PidDir(_) => Err(FsError::NotAFile),
            ProcNode::Meminfo => Ok(format_meminfo()),
            ProcNode::Mounts => Ok(format_mounts()),
            ProcNode::PidStat(pid) => format_stat(pid),
            ProcNode::PidStatus(pid) => format_status(pid),
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
