use super::*;
use core::sync::atomic::{AtomicU64, Ordering};

/// `/proc/sys/kernel/random/uuid` 的每次读取序号。
///
/// WaterOS 目前没有可复用的内核熵池 API；这里把调度 tick 与单调序号经过
/// SplitMix64 扩散，保证同一次启动中的每次读取都产生不同、格式和版本位正确的
/// UUID。它适合接口兼容与临时标识，不承诺密码学随机性。
static UUID_SEQUENCE : AtomicU64 = AtomicU64::new(1);

fn mix_uuid_word(mut value : u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn random_uuid() -> Vec<u8> {
    let sequence = UUID_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let tick = task::current_tick();
    let high = mix_uuid_word(sequence ^ tick.rotate_left(17));
    let low = mix_uuid_word(sequence.rotate_left(31) ^ tick ^ 0x5741_5445_524f_5300);
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&high.to_be_bytes());
    bytes[8..].copy_from_slice(&low.to_be_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // RFC 4122 version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // RFC 4122 variant
    format!("{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-\
             {:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}\n",
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7],
            bytes[8], bytes[9], bytes[10], bytes[11],
            bytes[12], bytes[13], bytes[14], bytes[15]).into_bytes()
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
            ProcNode::Stat |
            ProcNode::Loadavg |
            ProcNode::Version |
            ProcNode::Filesystems |
            ProcNode::Devices |
            ProcNode::Swaps |
            ProcNode::Partitions |
            ProcNode::Interrupts |
            ProcNode::Cmdline |
            ProcNode::Vmstat |
            ProcNode::Diskstats |
            ProcNode::Uptime |
            ProcNode::Cgroups |
            ProcNode::Mounts |
            ProcNode::NetDir |
            ProcNode::PressureDir |
            ProcNode::SysVIpcDir |
            ProcNode::SysDir |
            ProcNode::SysKernelDir |
            ProcNode::SysKernelRandomDir |
            ProcNode::SysVmDir |
            ProcNode::SysFsDir |
            ProcNode::SysNetDir |
            ProcNode::SysNetCoreDir |
            ProcNode::SysNetIpv4Dir => true,
            ProcNode::ProcNetTcp |
            ProcNode::ProcNetTcp6 |
            ProcNode::ProcNetUdp |
            ProcNode::ProcNetUdp6 |
            ProcNode::ProcNetRaw |
            ProcNode::ProcNetRaw6 |
            ProcNode::ProcNetUnix |
            ProcNode::ProcNetDev |
            ProcNode::ProcNetRoute |
            ProcNode::ProcNetSockstat |
            ProcNode::ProcNetSockstat6 |
            ProcNode::PressureCpu |
            ProcNode::PressureIo |
            ProcNode::PressureMemory |
            ProcNode::SysVIpcShm |
            ProcNode::SysVIpcMsg |
            ProcNode::SysVIpcSem => true,
            ProcNode::SysKernelPidMax | ProcNode::SysKernelTainted |
            ProcNode::SysKernelCapLastCap |
            ProcNode::SysKernelOsType |
            ProcNode::SysKernelOsRelease |
            ProcNode::SysKernelVersion |
            ProcNode::SysKernelHostname |
            ProcNode::SysKernelDomainname |
            ProcNode::SysKernelThreadsMax |
            ProcNode::SysKernelNgroupsMax |
            ProcNode::SysKernelShmMax |
            ProcNode::SysKernelShmAll |
            ProcNode::SysKernelShmMni |
            ProcNode::SysKernelShmRmidForced |
            ProcNode::SysVmOvercommitMemory |
            ProcNode::SysVmMaxMapCount |
            ProcNode::SysVmMmapMinAddr |
            ProcNode::SysFsFileMax |
            ProcNode::SysFsNrOpen |
            ProcNode::SysFsPipeMaxSize |
            ProcNode::SysFsFileNr |
            ProcNode::SysFsAioMaxNr |
            ProcNode::SysNetCoreSomaxconn |
            ProcNode::SysNetIpv4PortRange |
            ProcNode::SysNetIpv4TcpSyncookies |
            ProcNode::SysKernelRandomBootId |
            ProcNode::SysKernelRandomUuid |
            ProcNode::SysKernelRandomizeVaSpace |
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
            ProcNode::PidEnviron(pid) |
            ProcNode::PidAuxv(pid) |
            ProcNode::PidIo(pid) |
            ProcNode::PidSched(pid) |
            ProcNode::PidStatm(pid) |
            ProcNode::PidLimits(pid) |
            ProcNode::PidMounts(pid) |
            ProcNode::PidMountinfo(pid) |
            ProcNode::PidCgroup(pid) |
            ProcNode::PidWchan(pid) |
            ProcNode::PidExe(pid) |
            ProcNode::PidCwd(pid) |
            ProcNode::PidRoot(pid) |
            ProcNode::PidFdDir(pid) |
            ProcNode::PidFdInfoDir(pid) |
            ProcNode::PidNsDir(pid) |
            ProcNode::PidTaskRoot(pid) => process_visible(pid),
            ProcNode::PidNamespace(pid, _) => process_visible(pid),
            ProcNode::PidTaskDir(pid, _) |
            ProcNode::PidTaskComm(pid, _) |
            ProcNode::PidTaskStat(pid, _) |
            ProcNode::PidTaskStatus(pid, _) |
            ProcNode::PidTaskWchan(pid, _) => process_visible(pid),
            ProcNode::PidTaskSched(pid, _) => process_visible(pid),
            ProcNode::PidFd(pid, fd) => {
                task::leader_task_for_process(pid)
                    .map(|leader| fds_for(leader).contains(&fd))
                    .unwrap_or(false)
            }
            ProcNode::PidFdInfo(pid, fd) => task::leader_task_for_process(pid)
                .map(|leader| fds_for(leader).contains(&fd))
                .unwrap_or(false),
        })
    }

    // 本方法代码由AI完成
    fn metadata(&self, rel_path : &str) -> FsResult<FsMetadata> {
        let node = parse_node(rel_path).ok_or(FsError::NotFound)?;
        match node {
            ProcNode::Root |
            ProcNode::NetDir |
            ProcNode::PressureDir |
            ProcNode::SysVIpcDir |
            ProcNode::SysDir |
            ProcNode::SysKernelDir |
            ProcNode::SysKernelRandomDir |
            ProcNode::SysVmDir |
            ProcNode::SysFsDir |
            ProcNode::SysNetDir |
            ProcNode::SysNetCoreDir |
            ProcNode::SysNetIpv4Dir |
            ProcNode::PidDir(_) |
            ProcNode::PidFdDir(_) |
            ProcNode::PidFdInfoDir(_) |
            ProcNode::PidNsDir(_) |
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
            ProcNode::Cmdline |
            ProcNode::Vmstat |
            ProcNode::Diskstats |
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
            ProcNode::ProcNetDev |
            ProcNode::ProcNetRoute |
            ProcNode::ProcNetSockstat |
            ProcNode::ProcNetSockstat6 |
            ProcNode::PressureCpu |
            ProcNode::PressureIo |
            ProcNode::PressureMemory |
            ProcNode::SysVIpcShm |
            ProcNode::SysVIpcMsg |
            ProcNode::SysVIpcSem |
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
            ProcNode::SysKernelShmMax |
            ProcNode::SysKernelShmAll |
            ProcNode::SysKernelShmMni |
            ProcNode::SysKernelShmRmidForced |
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
            ProcNode::SysFsFileNr |
            ProcNode::SysFsAioMaxNr |
            ProcNode::SysNetCoreSomaxconn |
            ProcNode::SysNetIpv4PortRange |
            ProcNode::SysNetIpv4TcpSyncookies |
            ProcNode::SysKernelRandomBootId |
            ProcNode::SysKernelRandomUuid |
            ProcNode::SysKernelRandomizeVaSpace => Ok(FsMetadata { node_type : FsNodeType::File,
                                                          size : self.read(rel_path)?.len() as u64,
                                                          mode : 0o444,
                                                          inode : proc_inode(node), nlink : 1, uid : 0, gid : 0 }),
            ProcNode::PidEnviron(pid) | ProcNode::PidAuxv(pid) => {
                if !process_visible(pid) {
                    return Err(FsError::NotFound);
                }
                Ok(FsMetadata { node_type : FsNodeType::File,
                                size : self.read(rel_path)?.len() as u64,
                                mode : 0o400,
                                inode : proc_inode(node),
                                nlink : 1,
                                uid : 0,
                                gid : 0 })
            }
            ProcNode::PidStat(pid) |
            ProcNode::PidStatus(pid) |
            ProcNode::PidComm(pid) |
            ProcNode::PidTimerSlack(pid) |
            ProcNode::PidSmaps(pid) |
            ProcNode::PidMaps(pid) |
            ProcNode::PidCmdline(pid) |
            ProcNode::PidIo(pid) |
            ProcNode::PidSched(pid) |
            ProcNode::PidStatm(pid) |
            ProcNode::PidLimits(pid) |
            ProcNode::PidMounts(pid) |
            ProcNode::PidMountinfo(pid) |
            ProcNode::PidCgroup(pid) |
            ProcNode::PidWchan(pid) |
            ProcNode::PidFdInfo(pid, _) |
            ProcNode::PidTaskComm(pid, _) |
            ProcNode::PidTaskStat(pid, _) |
            ProcNode::PidTaskStatus(pid, _) |
            ProcNode::PidTaskWchan(pid, _) |
            ProcNode::PidTaskSched(pid, _) => {
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
            ProcNode::PidExe(pid) | ProcNode::PidCwd(pid) | ProcNode::PidRoot(pid) |
            ProcNode::PidFd(pid, _) | ProcNode::PidNamespace(pid, _) => {
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
            ProcNode::SysVIpcDir |
            ProcNode::SysKernelDir |
            ProcNode::SysVmDir |
            ProcNode::SysFsDir |
            ProcNode::SysNetDir |
            ProcNode::SysNetCoreDir |
            ProcNode::SysNetIpv4Dir |
            ProcNode::SysKernelRandomDir |
            ProcNode::PressureDir |
            ProcNode::PidDir(_) |
            ProcNode::PidFdDir(_) |
            ProcNode::PidFdInfoDir(_) |
            ProcNode::PidNsDir(_) |
            ProcNode::PidTaskRoot(_) |
            ProcNode::PidTaskDir(_, _) |
            ProcNode::NetDir |
            ProcNode::PidExe(_) |
            ProcNode::PidCwd(_) |
            ProcNode::PidRoot(_) |
            ProcNode::PidFd(_, _) |
            ProcNode::PidNamespace(_, _) => {
                Err(FsError::NotAFile)
            }
            ProcNode::ProcNetTcp => Ok(format_proc_net_table(network::SocketDomain::Ipv4,
                                                              SocketKind::Tcp)),
            ProcNode::ProcNetUdp => Ok(format_proc_net_table(network::SocketDomain::Ipv4,
                                                              SocketKind::Udp)),
            ProcNode::ProcNetTcp6 => Ok(format_proc_net_table(network::SocketDomain::Ipv6,
                                                               SocketKind::Tcp)),
            ProcNode::ProcNetUdp6 => Ok(format_proc_net_table(network::SocketDomain::Ipv6,
                                                               SocketKind::Udp)),
            ProcNode::ProcNetRaw |
            ProcNode::ProcNetRaw6 => Ok(PROC_NET_TABLE.to_vec()),
            ProcNode::ProcNetUnix => Ok(PROC_NET_UNIX_TABLE.to_vec()),
            ProcNode::ProcNetDev => Ok(format_proc_net_dev()),
            ProcNode::ProcNetRoute => Ok(format_proc_net_route()),
            ProcNode::ProcNetSockstat => Ok(format_sockstat(false)),
            ProcNode::ProcNetSockstat6 => Ok(format_sockstat(true)),
            ProcNode::PressureCpu => Ok(format_pressure(false)),
            ProcNode::PressureIo | ProcNode::PressureMemory => Ok(format_pressure(true)),
            ProcNode::SysVIpcShm => Ok(sysvipc_table(SysVIpcTable::Shm)),
            ProcNode::SysVIpcMsg => Ok(sysvipc_table(SysVIpcTable::Msg)),
            ProcNode::SysVIpcSem => Ok(sysvipc_table(SysVIpcTable::Sem)),
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
            ProcNode::Cmdline => Ok(b"\n".to_vec()),
            ProcNode::Vmstat => Ok(format_vmstat()),
            ProcNode::Diskstats => Ok(format_diskstats()),
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
            // WaterOS 当前单段最多 4 MiB、最多 4096 段；shmall 的单位是页。
            ProcNode::SysKernelShmMax => Ok(b"4194304\n".to_vec()),
            ProcNode::SysKernelShmAll => Ok(b"4194304\n".to_vec()),
            ProcNode::SysKernelShmMni => Ok(b"4096\n".to_vec()),
            ProcNode::SysKernelShmRmidForced => Ok(b"0\n".to_vec()),
            ProcNode::SysVmOvercommitMemory => Ok(b"0\n".to_vec()),
            ProcNode::SysVmMaxMapCount => Ok(b"65530\n".to_vec()),
            ProcNode::SysVmMmapMinAddr => Ok(b"65536\n".to_vec()),
            ProcNode::SysFsFileMax => Ok(b"9223372036854775807\n".to_vec()),
            ProcNode::SysFsNrOpen => Ok(b"1048576\n".to_vec()),
            ProcNode::SysFsPipeMaxSize => Ok(b"1048576\n".to_vec()),
            ProcNode::SysFsFileNr => Ok(b"0\t0\t9223372036854775807\n".to_vec()),
            ProcNode::SysFsAioMaxNr => Ok(b"65536\n".to_vec()),
            ProcNode::SysNetCoreSomaxconn => Ok(b"4096\n".to_vec()),
            ProcNode::SysNetIpv4PortRange => Ok(b"32768\t60999\n".to_vec()),
            ProcNode::SysNetIpv4TcpSyncookies => Ok(b"1\n".to_vec()),
            // boot_id 在单次启动内保持稳定；random/uuid 每次读取生成新值。
            ProcNode::SysKernelRandomBootId => Ok(b"57415445-524f-532d-424f-4f5449440001\n".to_vec()),
            ProcNode::SysKernelRandomUuid => Ok(random_uuid()),
            ProcNode::SysKernelRandomizeVaSpace => Ok(b"0\n".to_vec()),
            ProcNode::PidStat(pid) => format_stat(pid),
            ProcNode::PidStatus(pid) => format_status(pid),
            ProcNode::PidComm(pid) => format_pid_comm(pid),
            ProcNode::PidTimerSlack(pid) => format_pid_timer_slack(pid),
            ProcNode::PidSmaps(pid) => format_smaps(pid),
            ProcNode::PidMaps(pid) => format_maps(pid),
            ProcNode::PidCmdline(pid) => format_cmdline(pid),
            ProcNode::PidEnviron(pid) => format_environ(pid),
            ProcNode::PidAuxv(pid) => format_auxv(pid),
            ProcNode::PidIo(pid) => format_pid_io(pid),
            ProcNode::PidSched(pid) => format_sched(pid),
            ProcNode::PidStatm(pid) => format_statm(pid),
            ProcNode::PidLimits(pid) => format_limits(pid),
            ProcNode::PidMounts(pid) => {
                process_visible(pid).then(format_mounts).ok_or(FsError::NotFound)
            }
            ProcNode::PidMountinfo(pid) => format_mountinfo(pid),
            // cgroup controller/cgroupfs 尚未实现；空文件表示该进程未加入
            // 任何可见层级，比伪造 `0::/` 更准确。
            ProcNode::PidCgroup(pid) => {
                process_visible(pid).then(Vec::new).ok_or(FsError::NotFound)
            }
            ProcNode::PidWchan(pid) => format_wchan(pid),
            ProcNode::PidFdInfo(pid, fd) => format_fdinfo(pid, fd),
            ProcNode::PidTaskComm(pid, task_id) => format_task_comm(pid, task_id),
            ProcNode::PidTaskStat(pid, task_id) => format_task_stat(pid, task_id),
            ProcNode::PidTaskStatus(pid, task_id) => format_task_status(pid, task_id),
            ProcNode::PidTaskWchan(pid, task_id) => format_task_wchan(pid, task_id),
            ProcNode::PidTaskSched(pid, task_id) => format_task_sched(pid, task_id),
            ProcNode::SelfLink | ProcNode::ThreadSelfLink => Err(FsError::NotAFile),
        }
    }

    fn read_range(&self, rel_path : &str, offset : u64, buf : &mut [u8]) -> FsResult<usize> {
        let node = parse_node(rel_path).ok_or(FsError::NotFound)?;
        let static_data : &[u8] = match node {
            ProcNode::ProcNetRaw |
            ProcNode::ProcNetRaw6 => PROC_NET_TABLE,
            ProcNode::ProcNetUnix => PROC_NET_UNIX_TABLE,
            ProcNode::ProcNetTcp |
            ProcNode::ProcNetUdp |
            ProcNode::ProcNetTcp6 |
            ProcNode::ProcNetUdp6 => {
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
            ProcNode::PidCwd(pid) => {
                let leader = task::leader_task_for_process(pid).ok_or(FsError::NotFound)?;
                cwd_for(leader).map(String::into_bytes).ok_or(FsError::NotFound)
            }
            ProcNode::PidRoot(pid) => {
                let leader = task::leader_task_for_process(pid).ok_or(FsError::NotFound)?;
                root_for(leader).map(String::into_bytes).ok_or(FsError::NotFound)
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
            ProcNode::PidNamespace(pid, namespace) => {
                if !process_visible(pid) {
                    return Err(FsError::NotFound);
                }
                Ok(format!("{}:[{}]", namespace.name(), namespace.inode()).into_bytes())
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
                                       FsDirEntry { name : String::from("cmdline"), node_type : FsNodeType::File },
                                       FsDirEntry { name : String::from("vmstat"), node_type : FsNodeType::File },
                                       FsDirEntry { name : String::from("diskstats"), node_type : FsNodeType::File },
                                       FsDirEntry { name : String::from("uptime"),
                                                    node_type : FsNodeType::File },
                                       FsDirEntry { name : String::from("cgroups"),
                                                    node_type : FsNodeType::File },
                                       FsDirEntry { name : String::from("mounts"),
                                                    node_type : FsNodeType::File },
                                       FsDirEntry { name : String::from("net"),
                                                    node_type : FsNodeType::Directory },
                                       FsDirEntry { name : String::from("pressure"),
                                                    node_type : FsNodeType::Directory },
                                       FsDirEntry { name : String::from("sysvipc"),
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
                                             FsDirEntry { name : String::from("fs"), node_type : FsNodeType::Directory },
                                             FsDirEntry { name : String::from("net"), node_type : FsNodeType::Directory }]),
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
                        FsDirEntry { name : String::from("ngroups_max"), node_type : FsNodeType::File },
                        FsDirEntry { name : String::from("shmmax"), node_type : FsNodeType::File },
                        FsDirEntry { name : String::from("shmall"), node_type : FsNodeType::File },
                        FsDirEntry { name : String::from("shmmni"), node_type : FsNodeType::File },
                        FsDirEntry { name : String::from("shm_rmid_forced"), node_type : FsNodeType::File },
                        FsDirEntry { name : String::from("random"), node_type : FsNodeType::Directory },
                        FsDirEntry { name : String::from("randomize_va_space"), node_type : FsNodeType::File }])
            }
            ProcNode::SysKernelRandomDir => Ok(vec![
                FsDirEntry { name: String::from("boot_id"), node_type: FsNodeType::File },
                FsDirEntry { name: String::from("uuid"), node_type: FsNodeType::File },
            ]),
            ProcNode::SysVmDir => Ok(vec![FsDirEntry { name: String::from("overcommit_memory"), node_type: FsNodeType::File },
                                              FsDirEntry { name: String::from("max_map_count"), node_type: FsNodeType::File },
                                              FsDirEntry { name: String::from("mmap_min_addr"), node_type: FsNodeType::File }]),
            ProcNode::SysFsDir => Ok(vec![FsDirEntry { name: String::from("file-max"), node_type: FsNodeType::File },
                                              FsDirEntry { name: String::from("nr_open"), node_type: FsNodeType::File },
                                              FsDirEntry { name: String::from("pipe-max-size"), node_type: FsNodeType::File },
                                              FsDirEntry { name: String::from("file-nr"), node_type: FsNodeType::File },
                                              FsDirEntry { name: String::from("aio-max-nr"), node_type: FsNodeType::File }]),
            ProcNode::SysNetDir => Ok(vec![
                FsDirEntry { name: String::from("core"), node_type: FsNodeType::Directory },
                FsDirEntry { name: String::from("ipv4"), node_type: FsNodeType::Directory },
            ]),
            ProcNode::SysNetCoreDir => Ok(vec![FsDirEntry { name: String::from("somaxconn"), node_type: FsNodeType::File }]),
            ProcNode::SysNetIpv4Dir => Ok(vec![
                FsDirEntry { name: String::from("ip_local_port_range"), node_type: FsNodeType::File },
                FsDirEntry { name: String::from("tcp_syncookies"), node_type: FsNodeType::File },
            ]),
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
                FsDirEntry { name : String::from("dev"), node_type : FsNodeType::File },
                FsDirEntry { name : String::from("route"), node_type : FsNodeType::File },
                FsDirEntry { name : String::from("sockstat"), node_type : FsNodeType::File },
                FsDirEntry { name : String::from("sockstat6"), node_type : FsNodeType::File },
            ]),
            ProcNode::PressureDir => Ok(vec![
                FsDirEntry { name: String::from("cpu"), node_type: FsNodeType::File },
                FsDirEntry { name: String::from("io"), node_type: FsNodeType::File },
                FsDirEntry { name: String::from("memory"), node_type: FsNodeType::File },
            ]),
            ProcNode::SysVIpcDir => Ok(vec![
                FsDirEntry { name : String::from("shm"), node_type : FsNodeType::File },
                FsDirEntry { name : String::from("msg"), node_type : FsNodeType::File },
                FsDirEntry { name : String::from("sem"), node_type : FsNodeType::File },
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
                        FsDirEntry { name: String::from("environ"), node_type: FsNodeType::File },
                        FsDirEntry { name: String::from("auxv"), node_type: FsNodeType::File },
                        FsDirEntry { name: String::from("io"), node_type: FsNodeType::File },
                        FsDirEntry { name: String::from("statm"), node_type: FsNodeType::File },
                        FsDirEntry { name: String::from("limits"), node_type: FsNodeType::File },
                        FsDirEntry { name: String::from("mountinfo"), node_type: FsNodeType::File },
                        FsDirEntry { name: String::from("wchan"), node_type: FsNodeType::File },
                        FsDirEntry { name: String::from("sched"), node_type: FsNodeType::File },
                        FsDirEntry { name: String::from("cgroup"), node_type: FsNodeType::File },
                        FsDirEntry { name:
                                         String::from("exe"),
                                     node_type:
                                         FsNodeType::Symlink },
                        FsDirEntry { name: String::from("cwd"), node_type: FsNodeType::Symlink },
                        FsDirEntry { name: String::from("root"), node_type: FsNodeType::Symlink },
                        FsDirEntry { name:
                                         String::from("fd"),
                                     node_type:
                                         FsNodeType::Directory },
                        FsDirEntry { name: String::from("fdinfo"), node_type: FsNodeType::Directory },
                        FsDirEntry { name: String::from("ns"), node_type: FsNodeType::Directory },
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
            ProcNode::PidFdInfoDir(pid) => {
                let leader = task::leader_task_for_process(pid).ok_or(FsError::NotFound)?;
                Ok(fds_for(leader).into_iter().map(|fd| FsDirEntry {
                    name: fd.to_string(), node_type: FsNodeType::File,
                }).collect())
            }
            ProcNode::PidNsDir(pid) => {
                if !process_visible(pid) {
                    return Err(FsError::NotFound);
                }
                Ok(ProcNamespace::ALL
                    .into_iter()
                    .map(|namespace| FsDirEntry { name : String::from(namespace.name()),
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
                                     node_type : FsNodeType::File },
                        FsDirEntry { name : String::from("stat"),
                                     node_type : FsNodeType::File },
                        FsDirEntry { name : String::from("status"),
                                     node_type : FsNodeType::File },
                        FsDirEntry { name : String::from("wchan"),
                                     node_type : FsNodeType::File },
                        FsDirEntry { name : String::from("sched"),
                                     node_type : FsNodeType::File }])
            }
            _ => Err(FsError::NotAFile),
        }
    }
}
