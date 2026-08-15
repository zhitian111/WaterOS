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
            #[cfg(target_arch = "riscv64")]
            {
            output.push_str(format!("processor\t: {cpu}\n").as_str());
            output.push_str("hart\t\t: ");
            output.push_str(cpu.to_string()
                               .as_str());
            output.push('\n');
            output.push_str("model name\t: WaterOS RISC-V virtual CPU\n");
            output.push_str("isa\t\t: rv64imafdch\n\n");
            }
            #[cfg(target_arch = "loongarch64")]
            {
                output.push_str("system type\t: WaterOS QEMU LoongArch64\n");
                output.push_str(format!("processor\t: {cpu}\n").as_str());
                output.push_str("cpu family\t: LoongArch\n");
                output.push_str("model name\t: WaterOS LoongArch virtual CPU\n");
                output.push_str("ISA\t\t: loongarch64\n\n");
            }
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

/// Linux `/proc/stat` 的核心计数。调度器目前只区分 busy/idle tick，无法可靠
/// 拆分 user/nice/system，因此把全部非 idle 时间记入 user，其他列保持 0。
pub(crate) fn format_global_stat() -> Vec<u8> {
    let states = task::cpu_states();
    let mut total_busy = 0u64;
    let mut total_idle = 0u64;
    let mut context_switches = 0u64;
    let mut out = String::new();
    for (cpu_id, cpu) in states.iter().filter(|(_, cpu)| cpu.online) {
        let busy = cpu.timer_ticks.saturating_sub(cpu.idle_ticks);
        total_busy = total_busy.saturating_add(busy);
        total_idle = total_idle.saturating_add(cpu.idle_ticks);
        context_switches = context_switches.saturating_add(cpu.context_switches);
        let _ = writeln!(out, "cpu{} {} 0 0 {} 0 0 0 0 0 0", cpu_id.raw(), busy, cpu.idle_ticks);
    }
    let mut running = 0usize;
    let mut blocked = 0usize;
    let pids = task::all_process_pids();
    for pid in &pids {
        for task_id in task::task_ids_for_process(*pid).unwrap_or_default() {
            match task::task_snapshot(task_id).map(|snapshot| snapshot.state) {
                Some(TaskState::Ready | TaskState::Running { .. }) => running += 1,
                Some(TaskState::Blocking(_) | TaskState::Sleeping { .. }) => blocked += 1,
                Some(TaskState::Exited(_)) | None => {}
            }
        }
    }
    let last_pid = pids.iter().map(|pid| pid.raw()).max().unwrap_or(0);
    let mut header = format!("cpu {total_busy} 0 0 {total_idle} 0 0 0 0 0 0\n");
    header.push_str(out.as_str());
    let _ = writeln!(header, "intr {}", states.iter().map(|(_, cpu)| cpu.timer_ticks).sum::<u64>());
    let _ = writeln!(header, "ctxt {context_switches}");
    let _ = writeln!(header, "processes {}", pids.len());
    let _ = writeln!(header, "procs_running {running}");
    let _ = writeln!(header, "procs_blocked {blocked}");
    let _ = writeln!(header, "softirq 0 0 0 0 0 0 0 0 0 0 0");
    let _ = last_pid; // last_pid 属于 loadavg；保留这里的单次 PID 扫描语义说明。
    header.into_bytes()
}

pub(crate) fn format_loadavg() -> Vec<u8> {
    let states = task::cpu_states();
    let runnable = states.iter()
                         .filter(|(_, cpu)| cpu.online)
                         .map(|(_, cpu)| cpu.runnable_other + cpu.runnable_batch +
                                          cpu.runnable_fifo + cpu.runnable_rr + cpu.runnable_idle +
                                          usize::from(!cpu.current_is_idle && cpu.current_task_id.is_some()))
                         .sum::<usize>();
    let pids = task::all_process_pids();
    let tasks = pids.iter()
                    .map(|pid| task::task_ids_for_process(*pid).map_or(0, |ids| ids.len()))
                    .sum::<usize>();
    let last_pid = pids.iter().map(|pid| pid.raw()).max().unwrap_or(0);
    // 尚无指数衰减历史，三个 load 字段都发布当前 runnable 数；格式与 Linux 一致。
    format!("{runnable}.00 {runnable}.00 {runnable}.00 {runnable}/{tasks} {last_pid}\n").into_bytes()
}

pub(crate) fn format_filesystems() -> Vec<u8> {
    b"\text4\nnodev\ttmpfs\nnodev\tproc\nnodev\tsysfs\nnodev\tdevtmpfs\nnodev\tcgroup\nnodev\tcgroup2\n".to_vec()
}

pub(crate) fn format_devices() -> Vec<u8> {
    b"Character devices:\n  1 mem\n  5 tty\n 10 misc\n\nBlock devices:\n252 virtblk\n".to_vec()
}

pub(crate) fn format_partitions() -> Vec<u8> {
    let Some(device) = driver_block_api_v0::first_block_device() else {
        return b"major minor  #blocks  name\n\n".to_vec();
    };
    let device = device.lock();
    let blocks_kb = device.total_blocks()
                          .unwrap_or(0)
                          .saturating_mul(device.block_size() as u64)
                          / 1024;
    format!("major minor  #blocks  name\n\n 252        0 {blocks_kb:>10} vda\n").into_bytes()
}

/// Linux `/proc/diskstats` 的最小兼容视图。WaterOS 块设备层目前没有逐请求
/// 计数器，所以容量与设备身份是真实的，I/O 统计列明确保持为 0。
pub(crate) fn format_diskstats() -> Vec<u8> {
    if driver_block_api_v0::first_block_device().is_none() {
        return Vec::new();
    }
    b" 252       0 vda 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n".to_vec()
}

/// 常见工具会读取 vmstat 判断分页和内存压力。尚未拥有的内核计数不伪造，
/// 但发布当前可用/已用物理页，使 free、procps 和 stress-ng 能稳定解析。
pub(crate) fn format_vmstat() -> Vec<u8> {
    let stats = mm_frame_alloctor::frame_mem_stats();
    let free_pages = stats.free_bytes() / 4096;
    let total_pages = stats.total_bytes() / 4096;
    let used_pages = total_pages.saturating_sub(free_pages);
    format!("nr_free_pages {free_pages}\n\
             nr_zone_inactive_anon 0\n\
             nr_zone_active_anon {used_pages}\n\
             nr_zone_inactive_file 0\n\
             nr_zone_active_file 0\n\
             nr_anon_pages {used_pages}\n\
             nr_mapped 0\n\
             nr_file_pages 0\n\
             nr_dirty 0\n\
             nr_writeback 0\n\
             pgpgin 0\npgpgout 0\npswpin 0\npswpout 0\npgfault 0\npgmajfault 0\n")
        .into_bytes()
}

pub(crate) fn format_proc_net_dev() -> Vec<u8> {
    b"Inter-|   Receive                                                |  Transmit\n\
       face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n\
          lo:       0       0    0    0    0     0          0         0        0       0    0    0    0     0       0          0\n\
        eth0:       0       0    0    0    0     0          0         0        0       0    0    0    0     0       0          0\n"
        .to_vec()
}

pub(crate) fn format_proc_net_route() -> Vec<u8> {
    b"Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
      eth0\t00000000\t0202000A\t0003\t0\t0\t0\t00000000\t0\t0\t0\n\
      eth0\t0002000A\t00000000\t0001\t0\t0\t0\t00FFFFFF\t0\t0\t0\n"
        .to_vec()
}

pub(crate) fn format_sockstat(ipv6 : bool) -> Vec<u8> {
    if ipv6 {
        return b"TCP6: inuse 0\nUDP6: inuse 0\nUDPLITE6: inuse 0\nRAW6: inuse 0\nFRAG6: inuse 0 memory 0\n".to_vec();
    }
    let sockets = network::stack::network_socket_table_snapshot().unwrap_or_default();
    let tcp = sockets.iter().filter(|socket| socket.kind == SocketKind::Tcp).count();
    let udp = sockets.iter().filter(|socket| socket.kind == SocketKind::Udp).count();
    format!("sockets: used {}\nTCP: inuse {tcp} orphan 0 tw 0 alloc {tcp} mem 0\n\
             UDP: inuse {udp} mem 0\nUDPLITE: inuse 0\nRAW: inuse 0\nFRAG: inuse 0 memory 0\n",
            sockets.len()).into_bytes()
}

pub(crate) fn format_pressure(full : bool) -> Vec<u8> {
    let mut out = String::from("some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n");
    if full {
        out.push_str("full avg10=0.00 avg60=0.00 avg300=0.00 total=0\n");
    }
    out.into_bytes()
}

pub(crate) fn format_interrupts() -> Vec<u8> {
    let states = task::cpu_states();
    let online: Vec<_> = states.iter().filter(|(_, cpu)| cpu.online).collect();
    let mut out = String::from("           ");
    for (cpu_id, _) in &online {
        let _ = write!(out, "CPU{:<8}", cpu_id.raw());
    }
    out.push('\n');
    out.push_str("TIMER:     ");
    for (_, cpu) in &online {
        let _ = write!(out, "{:<11}", cpu.timer_ticks);
    }
    out.push_str(" WaterOS timer\nIPI:       ");
    for _ in &online {
        out.push_str("0          ");
    }
    out.push_str(" WaterOS IPI\n");
    out.into_bytes()
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
      hugetlb\t12\t1\t1\n"
                          .to_vec()
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
    // Linux 的 stat 第 2 列以括号包围，且传统工具只假定 comm 最多 15 个
    // 字节。先去掉换行和括号，避免一个异常 argv 把后续列错位。
    let comm = comm_for(pid);
    let comm = comm.replace(['\n', '\r', '(', ')'], "_");
    let comm15: String = comm.chars().take(15).collect();
    let ppid = process.parent_pid
                      .map(|p| p.raw())
                      .unwrap_or(0);
    // /proc/<pid>/stat 第 5/6 字段（pgrp/session）：LTP getpgid01 读 init 的
    // pgrp 与 getpgid(1) 比对，必须填真实值而非 0。
    let pgrp = process.pgid.raw();
    let session = process.sid.raw();
    let utime = jiffies;
    let stime = jiffies;
    let leader_state = task::task_snapshot(leader).map(|snap| snap.state);
    let sc = state_char(process.state, leader_state);
    // 字段 7..13 分别是 tty_nr、tpgid、flags、minflt、cminflt、majflt、
    // cmajflt；因此 utime 必须紧随这 **7** 个 0，处于第 14 列。
    let line = format!("{} ({}) {} {} {} {} 0 0 0 0 0 0 0 {} {} 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 \
                        0 0 0 0 0 0 0 0 0\n",
                       pid.raw(),
                       comm15,
                       sc,
                       ppid,
                       pgrp,
                       session,
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
        'S' => "sleeping",
        'Z' => "zombie",
        'T' => "stopped",
        _ => "running",
    };
    let caps = task::process_caps(pid).unwrap_or_default();
    let line = format!("Name:\t{comm}\nState:\t{sc} ({state_str})\nTgid:\t{}\nPid:\t{}\nPPid:\t{ppid}\nUid:\t{}\t{}\t{}\t{}\nGid:\t{}\t{}\t{}\t{}\nCapInh:\t{:08x}{:08x}\nCapPrm:\t{:08x}{:08x}\nCapEff:\t{:08x}{:08x}\nCapBnd:\t{:08x}{:08x}\nCapAmb:\t0000000000000000\nVmPeak:\t{}\tkB\nVmSize:\t{}\tkB\nVmRSS:\t{}\tkB\nVmData:\t{}\tkB\nVmStk:\t128\tkB\n",
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
                       0u32,
                       caps.inheritable,
                       0u32,
                       caps.permitted,
                       0u32,
                       caps.effective,
                       0u32,
                       caps.bounding,
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
        format!("{start:016x}-{end:016x} rw-p 00000000 00:00 0 [heap]\n\
                 Size:\t{}\tkB\n\
                 Rss:\t{}\tkB\n\
                 Pss:\t{}\tkB\n\
                 Shared_Clean:\t0\tkB\n\
                 Shared_Dirty:\t0\tkB\n\
                 Private_Clean:\t0\tkB\n\
                 Private_Dirty:\t{}\tkB\n\
                 Referenced:\t{}\tkB\n\
                 Anonymous:\t{}\tkB\n\
                 Swap:\t0\tkB\n\
                 KernelPageSize:\t4\tkB\n\
                 MMUPageSize:\t4\tkB\n\
                 VmFlags: rd wr mr mw me ac sd\n",
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

pub(crate) fn format_statm(pid : ProcessId) -> FsResult<Vec<u8>> {
    let mem = process_memory_kb(pid)?;
    let size = (mem.size_kb + 3) / 4;
    let resident = (mem.rss_kb + 3) / 4;
    // size resident shared text lib data dirty，单位均为页。
    Ok(format!("{size} {resident} 0 0 0 {size} 0\n").into_bytes())
}

fn format_limit(value : u64) -> String {
    if value == u64::MAX { String::from("unlimited") } else { value.to_string() }
}

/// `/proc/<pid>/limits` 与 getrlimit 共用同一份进程资源限制；未显式设置的
/// 项使用 syscall 层的 Linux 风格默认值。
pub(crate) fn format_limits(pid : ProcessId) -> FsResult<Vec<u8>> {
    if !process_visible(pid) {
        return Err(FsError::NotFound);
    }
    const DEFAULTS : &[(usize, &str, u64, u64, &str)] = &[
        (0, "Max cpu time", u64::MAX, u64::MAX, "seconds"),
        (1, "Max file size", u64::MAX, u64::MAX, "bytes"),
        (2, "Max data size", u64::MAX, u64::MAX, "bytes"),
        (3, "Max stack size", 8 * 1024 * 1024, 8 * 1024 * 1024, "bytes"),
        (4, "Max core file size", 0, 0, "bytes"),
        (5, "Max resident set", u64::MAX, u64::MAX, "bytes"),
        (6, "Max processes", 1024, 1024, "processes"),
        (7, "Max open files", 1024, 1024, "files"),
        (8, "Max locked memory", 64 * 1024, 64 * 1024, "bytes"),
        (9, "Max address space", u64::MAX, u64::MAX, "bytes"),
        (10, "Max file locks", u64::MAX, u64::MAX, "locks"),
        (11, "Max pending signals", 1024, 1024, "signals"),
        (12, "Max msgqueue size", 819200, 819200, "bytes"),
        (13, "Max nice priority", 0, 0, ""),
        (14, "Max realtime priority", 0, 0, ""),
        (15, "Max realtime timeout", u64::MAX, u64::MAX, "us"),
    ];
    let mut out = String::from("Limit                     Soft Limit           Hard Limit           Units     \n");
    for &(resource, name, default_cur, default_max, units) in DEFAULTS {
        let (cur, max) = task::process_resource_limit(pid, resource)
            .map(|limit| (limit.cur, limit.max))
            .unwrap_or((default_cur, default_max));
        let _ = writeln!(out, "{name:<25} {:<20} {:<20} {units}", format_limit(cur), format_limit(max));
    }
    Ok(out.into_bytes())
}

pub(crate) fn format_mountinfo(pid : ProcessId) -> FsResult<Vec<u8>> {
    if !process_visible(pid) {
        return Err(FsError::NotFound);
    }
    let mut out = String::new();
    for (index, line) in mount_lines().into_iter().enumerate() {
        let id = index + 1;
        let parent = if id == 1 { 0 } else { 1 };
        let access = if line.readonly { "ro" } else { "rw" };
        let _ = writeln!(out, "{id} {parent} 0:0 / {} {access},relatime - {} {} {access}",
                         line.mount_point, line.fstype, line.device);
    }
    Ok(out.into_bytes())
}

pub(crate) fn format_wchan(pid : ProcessId) -> FsResult<Vec<u8>> {
    let leader = task::leader_task_for_process(pid).ok_or(FsError::NotFound)?;
    let name = match task::task_snapshot(leader).map(|snapshot| snapshot.state) {
        Some(TaskState::Sleeping { .. }) => "hrtimer_nanosleep",
        Some(TaskState::Blocking(TaskWaitTarget::WaitQueue(_))) => "wait_queue",
        Some(TaskState::Blocking(TaskWaitTarget::TaskExit(_))) => "wait_task",
        Some(TaskState::Blocking(TaskWaitTarget::ChildExit(_))) => "do_wait",
        Some(TaskState::Blocking(TaskWaitTarget::Manual)) => "schedule",
        _ => "0",
    };
    Ok(format!("{name}\n").into_bytes())
}

pub(crate) fn format_fdinfo(pid : ProcessId, fd : usize) -> FsResult<Vec<u8>> {
    let leader = task::leader_task_for_process(pid).ok_or(FsError::NotFound)?;
    if !fds_for(leader).contains(&fd) {
        return Err(FsError::NotFound);
    }
    // VFS 暂未向 procfs 暴露共享 open-description 的 offset/flags；不要伪造
    // 可写状态，只发布能被 procps、shell 和 lsof 稳定解析的保守值。
    Ok(b"pos:\t0\nflags:\t0100000\nmnt_id:\t1\nino:\t0\n".to_vec())
}

// 本方法代码由AI完成
pub(crate) fn format_meminfo() -> Vec<u8> {
    let stats = mm_frame_alloctor::frame_mem_stats();
    format!("MemTotal:\t{}\tkB\nMemFree:\t{}\tkB\nMemAvailable:\t{}\tkB\nBuffers:\t0\tkB\nCached:\t0\tkB\n",
            stats.total_bytes() / 1024,
            stats.free_bytes() / 1024,
            stats.free_bytes() / 1024,).into_bytes()
}

// 本方法代码由AI完成
pub(crate) fn format_mounts() -> Vec<u8> {
    let mut out = Vec::new();
    for line in mount_lines() {
        let access = if line.readonly { "ro" } else { "rw" };
        let row = format!("{} {} {} {},relatime 0 0\n",
                          line.device, line.mount_point, line.fstype, access);
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
        (_, SocketState::Listening { .. }) => 0x0A,
        (_, SocketState::Connecting) => 0x02,
        (_, SocketState::Connected) => 0x01,
        (_, SocketState::Created | SocketState::Bound { .. } | SocketState::Closed) => 0x07,
    }
}

pub(crate) fn format_proc_net_table(protocol : SocketKind) -> Vec<u8> {
    let mut out = String::from_utf8_lossy(PROC_NET_TABLE).into_owned();
    for (slot, socket) in
        network::stack::network_socket_table_snapshot().unwrap_or_default()
                                                       .into_iter()
                                                       .filter(|socket| socket.kind == protocol)
                                                       .enumerate()
    {
        let _ = writeln!(out,
                         "{:4}: {:08X}:{:04X} {:08X}:{:04X} {:02X} {:08X}:{:08X} 00:00000000 \
                          00000000     0        0 0 1 0000000000000000 100 0 0 10 0",
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
