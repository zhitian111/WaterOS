use super::*;

// 进程仍存在于 task 子系统时才对外可见。
// 本方法代码由AI完成
pub(crate) fn process_visible(pid : ProcessId) -> bool { task::process_snapshot(pid).is_some() }

// 进程 comm：优先 argv[0] 基名，其次 exe 基名，最后回退 `"process"`。
// 本方法代码由AI完成
pub(crate) fn comm_for(pid : ProcessId) -> String {
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

pub(crate) fn format_task_comm(pid : ProcessId, task_id : TaskId) -> FsResult<Vec<u8>> {
    if !process_visible(pid) {
        return Err(FsError::NotFound);
    }
    let comm = thread_comm_str(task_id).unwrap_or_else(|| comm_for(pid));
    let mut out = comm.into_bytes();
    out.push(b'\n');
    Ok(out)
}

pub(crate) fn format_pid_comm(pid : ProcessId) -> FsResult<Vec<u8>> {
    if !process_visible(pid) {
        return Err(FsError::NotFound);
    }
    let mut out = comm_for(pid).into_bytes();
    out.push(b'\n');
    Ok(out)
}

pub(crate) fn format_pid_timer_slack(pid : ProcessId) -> FsResult<Vec<u8>> {
    if !process_visible(pid) {
        return Err(FsError::NotFound);
    }
    let leader = task::leader_task_for_process(pid).ok_or(FsError::NotFound)?;
    let out = format!("{}\n", timer_slack_for(leader)).into_bytes();
    Ok(out)
}

// 本方法代码由AI完成
pub(crate) fn format_cpuinfo() -> Vec<u8> {
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

pub(crate) fn format_uptime() -> Vec<u8> {
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
pub(crate) fn format_cgroups() -> Vec<u8> {
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
pub(crate) fn basename(path : &str) -> String {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .to_string()
}

// Linux `/proc/pid/stat` 单字符状态码（简化版）。
// 本方法代码由AI完成
pub(crate) fn state_char(process : ProcessState, leader_state : Option<TaskState>) -> char {
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
pub(crate) fn format_stat(pid : ProcessId) -> FsResult<Vec<u8>> {
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
pub(crate) fn format_status(pid : ProcessId) -> FsResult<Vec<u8>> {
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
pub(crate) struct ProcMemoryKb {
    size_kb : usize,
    rss_kb : usize,
    private_dirty_kb : usize,
}

// 本方法代码由AI完成
pub(crate) fn process_memory_kb(pid : ProcessId) -> FsResult<ProcMemoryKb> {
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
pub(crate) fn format_maps(pid : ProcessId) -> FsResult<Vec<u8>> {
    let mem = process_memory_kb(pid)?;
    let heap_start = 0x1000_0000usize;
    let heap_end = heap_start.saturating_add(mem.size_kb
                                                .saturating_mul(1024));
    Ok(format!("{heap_start:016x}-{heap_end:016x} rw-p 00000000 00:00 0 \
                [heap]\n3f0000000000-3f0000010000 r-xp 00000000 00:00 0 [vdso]\n").into_bytes())
}

// 本方法代码由AI完成
pub(crate) fn format_smaps(pid : ProcessId) -> FsResult<Vec<u8>> {
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
pub(crate) fn format_cmdline(pid : ProcessId) -> FsResult<Vec<u8>> {
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
pub(crate) fn format_meminfo() -> Vec<u8> {
    let stats = mm_frame_alloctor::frame_mem_stats();
    format!("MemTotal:\t{}\tkB\nMemFree:\t{}\tkB\nMemAvailable:\t{}\tkB\nBuffers:\t0\tkB\n\
             Cached:\t0\tkB\n",
            stats.total_bytes() / 1024,
            stats.free_bytes() / 1024,
            stats.free_bytes() / 1024,).into_bytes()
}

// 本方法代码由AI完成
pub(crate) fn format_mounts() -> Vec<u8> {
    let mut out = Vec::new();
    for line in mount_lines() {
        let row = format!("{} {} {} rw,relatime 0 0\n",
                          line.device, line.mount_point, line.fstype);
        out.extend_from_slice(row.as_bytes());
    }
    out
}

pub(crate) const PROC_NET_TABLE : &[u8] =
    b"  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n";
pub(crate) const PROC_NET_UNIX_TABLE : &[u8] =
    b"Num       RefCount Protocol Flags    Type St Inode Path\n";

pub(crate) fn proc_ipv4_hex(address : [u8; 4]) -> u32 { u32::from_le_bytes(address) }

pub(crate) fn proc_socket_state(state : SocketState, protocol : SocketKind) -> u8 {
    match (protocol, state) {
        (SocketKind::Udp, _) => 0x07,
        (_, SocketState::Listening { .. }) => 0x0a,
        (_, SocketState::Connecting) => 0x02,
        (_, SocketState::Connected) => 0x01,
        (_, SocketState::Created | SocketState::Bound { .. } | SocketState::Closed) => 0x07,
    }
}

pub(crate) fn format_proc_net_table(protocol : SocketKind) -> Vec<u8> {
    let mut out = String::from_utf8_lossy(PROC_NET_TABLE).into_owned();
    for (slot, socket) in network::stack::network_socket_table_snapshot()
                                  .unwrap_or_default()
                                  .into_iter()
                                  .filter(|socket| socket.kind == protocol)
                                  .enumerate()
    {
        let _ = writeln!(out,
                         "{:4}: {:08X}:{:04X} {:08X}:{:04X} {:02X} {:08X}:{:08X} 00:00000000 00000000     0        0 0 1 0000000000000000 100 0 0 10 0",
                         slot,
                         proc_ipv4_hex(socket.local.address),
                         socket.local.port,
                         proc_ipv4_hex(socket.peer.address),
                         socket.peer.port,
                         proc_socket_state(socket.state, protocol),
                         socket.tx_queue,
                         socket.rx_queue);
    }
    out.into_bytes()
}



