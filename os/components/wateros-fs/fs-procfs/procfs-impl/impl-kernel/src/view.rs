use super::*;

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
            ProcNode::Stat |
            ProcNode::Loadavg |
            ProcNode::Version |
            ProcNode::Filesystems |
            ProcNode::Devices |
            ProcNode::Swaps |
            ProcNode::Partitions |
            ProcNode::Interrupts |
            ProcNode::Uptime |
            ProcNode::Cgroups |
            ProcNode::Mounts |
            ProcNode::NetDir |
            ProcNode::SysDir |
            ProcNode::SysKernelDir |
            ProcNode::SysVmDir |
            ProcNode::SysFsDir => true,
            ProcNode::ProcNetTcp |
            ProcNode::ProcNetTcp6 |
            ProcNode::ProcNetUdp |
            ProcNode::ProcNetUdp6 |
            ProcNode::ProcNetRaw |
            ProcNode::ProcNetRaw6 |
            ProcNode::ProcNetUnix => true,
            ProcNode::SysKernelPidMax | ProcNode::SysKernelTainted |
            ProcNode::SysKernelCapLastCap |
            ProcNode::SysKernelOsType |
            ProcNode::SysKernelOsRelease |
            ProcNode::SysKernelVersion |
            ProcNode::SysKernelHostname |
            ProcNode::SysKernelDomainname |
            ProcNode::SysKernelThreadsMax |
            ProcNode::SysKernelNgroupsMax |
            ProcNode::SysVmOvercommitMemory |
            ProcNode::SysVmMaxMapCount |
            ProcNode::SysVmMmapMinAddr |
            ProcNode::SysFsFileMax |
            ProcNode::SysFsNrOpen |
            ProcNode::SysFsPipeMaxSize |
            ProcNode::SelfLink |
            ProcNode::ThreadSelfLink => true,
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
            ProcNode::SysVmDir |
            ProcNode::SysFsDir |
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
            ProcNode::Stat |
            ProcNode::Loadavg |
            ProcNode::Version |
            ProcNode::Filesystems |
            ProcNode::Devices |
            ProcNode::Swaps |
            ProcNode::Partitions |
            ProcNode::Interrupts |
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
            ProcNode::SysKernelTainted |
            ProcNode::SysKernelCapLastCap |
            ProcNode::SysKernelOsType |
            ProcNode::SysKernelOsRelease |
            ProcNode::SysKernelVersion |
            ProcNode::SysKernelHostname |
            ProcNode::SysKernelDomainname |
            ProcNode::SysKernelThreadsMax |
            ProcNode::SysKernelNgroupsMax |
            ProcNode::SysVmOvercommitMemory |
            ProcNode::SysVmMaxMapCount |
            ProcNode::SysVmMmapMinAddr |
            ProcNode::SysFsFileMax |
            ProcNode::SysFsNrOpen |
            ProcNode::SysFsPipeMaxSize => Ok(FsMetadata { node_type : FsNodeType::File,
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
            ProcNode::SelfLink | ProcNode::ThreadSelfLink => {
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
            ProcNode::SysVmDir |
            ProcNode::SysFsDir |
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
            ProcNode::Stat => Ok(format_global_stat()),
            ProcNode::Loadavg => Ok(format_loadavg()),
            ProcNode::Version => Ok(b"Linux version 6.6.0-wateros (WaterOS) #1 SMP\n".to_vec()),
            ProcNode::Filesystems => Ok(format_filesystems()),
            ProcNode::Devices => Ok(format_devices()),
            ProcNode::Swaps => Ok(b"Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n".to_vec()),
            ProcNode::Partitions => Ok(format_partitions()),
            ProcNode::Interrupts => Ok(format_interrupts()),
            ProcNode::Uptime => Ok(format_uptime()),
            ProcNode::Cgroups => Ok(format_cgroups()),
            ProcNode::Mounts => Ok(format_mounts()),
            ProcNode::SysKernelPidMax => Ok(b"32768\n".to_vec()),
            ProcNode::SysKernelTainted => Ok(b"0\n".to_vec()),
            // 与 task::ProcessCaps::CAP_LAST_CAP（WaterOS 只支持低 32 位
            // capability）保持一致；libcap 靠此探测 cap_last_cap。
            ProcNode::SysKernelCapLastCap => Ok(b"31\n".to_vec()),
            ProcNode::SysKernelOsType => Ok(b"Linux\n".to_vec()),
            ProcNode::SysKernelOsRelease => Ok(b"6.6.0-wateros\n".to_vec()),
            ProcNode::SysKernelVersion => Ok(b"#1 SMP WaterOS\n".to_vec()),
            ProcNode::SysKernelHostname => Ok(b"wateros\n".to_vec()),
            ProcNode::SysKernelDomainname => Ok(b"(none)\n".to_vec()),
            ProcNode::SysKernelThreadsMax => Ok(b"32768\n".to_vec()),
            ProcNode::SysKernelNgroupsMax => Ok(b"65536\n".to_vec()),
            ProcNode::SysVmOvercommitMemory => Ok(b"0\n".to_vec()),
            ProcNode::SysVmMaxMapCount => Ok(b"65530\n".to_vec()),
            ProcNode::SysVmMmapMinAddr => Ok(b"65536\n".to_vec()),
            ProcNode::SysFsFileMax => Ok(b"9223372036854775807\n".to_vec()),
            ProcNode::SysFsNrOpen => Ok(b"1048576\n".to_vec()),
            ProcNode::SysFsPipeMaxSize => Ok(b"1048576\n".to_vec()),
            ProcNode::PidStat(pid) => format_stat(pid),
            ProcNode::PidStatus(pid) => format_status(pid),
            ProcNode::PidComm(pid) => format_pid_comm(pid),
            ProcNode::PidTimerSlack(pid) => format_pid_timer_slack(pid),
            ProcNode::PidSmaps(pid) => format_smaps(pid),
            ProcNode::PidMaps(pid) => format_maps(pid),
            ProcNode::PidCmdline(pid) => format_cmdline(pid),
            ProcNode::PidTaskComm(pid, task_id) => format_task_comm(pid, task_id),
            ProcNode::SelfLink | ProcNode::ThreadSelfLink => Err(FsError::NotAFile),
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
            ProcNode::SysKernelCapLastCap => b"31\n",
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
            ProcNode::SelfLink => {
                let pid = task::current_process_task_snapshot().ok_or(FsError::NotFound)?.pid;
                Ok(pid.raw().to_string().into_bytes())
            }
            ProcNode::ThreadSelfLink => {
                let current = task::current_process_task_snapshot().ok_or(FsError::NotFound)?;
                Ok(format!("{}/task/{}", current.pid.raw(), current.tid.raw()).into_bytes())
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
                                       FsDirEntry { name : String::from("stat"), node_type : FsNodeType::File },
                                       FsDirEntry { name : String::from("loadavg"), node_type : FsNodeType::File },
                                       FsDirEntry { name : String::from("version"), node_type : FsNodeType::File },
                                       FsDirEntry { name : String::from("filesystems"), node_type : FsNodeType::File },
                                       FsDirEntry { name : String::from("devices"), node_type : FsNodeType::File },
                                       FsDirEntry { name : String::from("swaps"), node_type : FsNodeType::File },
                                       FsDirEntry { name : String::from("partitions"), node_type : FsNodeType::File },
                                       FsDirEntry { name : String::from("interrupts"), node_type : FsNodeType::File },
                                       FsDirEntry { name : String::from("uptime"),
                                                    node_type : FsNodeType::File },
                                       FsDirEntry { name : String::from("cgroups"),
                                                    node_type : FsNodeType::File },
                                       FsDirEntry { name : String::from("mounts"),
                                                    node_type : FsNodeType::File },
                                       FsDirEntry { name : String::from("net"),
                                                    node_type : FsNodeType::Directory },
                                       FsDirEntry { name : String::from("sys"),
                                                    node_type : FsNodeType::Directory },
                                       FsDirEntry { name : String::from("self"), node_type : FsNodeType::Symlink },
                                       FsDirEntry { name : String::from("thread-self"), node_type : FsNodeType::Symlink },];
                for pid in task::all_process_pids() {
                    entries.push(FsDirEntry { name : format!("{}", pid.raw()),
                                              node_type : FsNodeType::Directory });
                }
                Ok(entries)
            }
            ProcNode::SysDir => Ok(vec![FsDirEntry { name : String::from("kernel"),
                                                     node_type : FsNodeType::Directory },
                                             FsDirEntry { name : String::from("vm"), node_type : FsNodeType::Directory },
                                             FsDirEntry { name : String::from("fs"), node_type : FsNodeType::Directory }]),
            ProcNode::SysKernelDir => {
                Ok(vec![FsDirEntry { name : String::from("pid_max"),
                                     node_type : FsNodeType::File },
                        FsDirEntry { name : String::from("tainted"),
                                     node_type : FsNodeType::File },
                        FsDirEntry { name : String::from("cap_last_cap"),
                                     node_type : FsNodeType::File },
                        FsDirEntry { name : String::from("ostype"), node_type : FsNodeType::File },
                        FsDirEntry { name : String::from("osrelease"), node_type : FsNodeType::File },
                        FsDirEntry { name : String::from("version"), node_type : FsNodeType::File },
                        FsDirEntry { name : String::from("hostname"), node_type : FsNodeType::File },
                        FsDirEntry { name : String::from("domainname"), node_type : FsNodeType::File },
                        FsDirEntry { name : String::from("threads-max"), node_type : FsNodeType::File },
                        FsDirEntry { name : String::from("ngroups_max"), node_type : FsNodeType::File }])
            }
            ProcNode::SysVmDir => Ok(vec![FsDirEntry { name: String::from("overcommit_memory"), node_type: FsNodeType::File },
                                              FsDirEntry { name: String::from("max_map_count"), node_type: FsNodeType::File },
                                              FsDirEntry { name: String::from("mmap_min_addr"), node_type: FsNodeType::File }]),
            ProcNode::SysFsDir => Ok(vec![FsDirEntry { name: String::from("file-max"), node_type: FsNodeType::File },
                                              FsDirEntry { name: String::from("nr_open"), node_type: FsNodeType::File },
                                              FsDirEntry { name: String::from("pipe-max-size"), node_type: FsNodeType::File }]),
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
