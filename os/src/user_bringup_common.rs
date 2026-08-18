//! 用户态 bring-up 各阶段共享的 ELF 装载、spawn 与串行等待逻辑。

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use mm::api::kernel_bringup::{LoadProgramError, LoadedElf, LoadedProgram, PrepareUserStackError};
use runtime::logging::*;

const PROCESS_EXIT_GRACE_TICKS : task::TaskTick = 64;


/// bring-up 阶段一次用户态启动：`program` 为待装载 ELF，`argv` 为完整参数（busybox 时
/// `argv[0]` 须为 applet 名，如 `"sh"` / `"timeout"`，而非 busybox 路径）。
pub struct BringupCommand {
    /// 要装载并执行的 ELF 路径；路径必须存在于已挂载根卷。
    pub program : &'static str,
    /// 传给新任务的完整参数数组；`argv[0]` 遵循目标程序约定。
    pub argv : &'static [&'static str],
}

/// 按 [`BringupCommand`] 串行执行：装载 `program` → spawn → wait → reap。
pub fn run_one_bringup_command(log_tag : &str, cmd : &BringupCommand) -> Option<isize> {
    if cmd.argv.is_empty() {
        warn!("[{log_tag}] skip cmd program={}: empty argv",
              cmd.program);
        return None;
    }
    // PERF_PROBE: restore for a dedicated zeroed-frame-pool measurement build.
    // mm::frame_alloctor::reset_zeroed_frame_pool_stats();
    let result = run_one_elf_argv_exit(log_tag, cmd.program, cmd.argv);
    // PERF_PROBE: restore for a dedicated zeroed-frame-pool measurement build.
    // mm::frame_alloctor::log_zeroed_frame_pool_stats(cmd.program);
    result
}

/// 基于已装载 ELF 创建尚未发布的用户任务，并在用户栈上写入 `argv` / `envp`
///（与 `execve` 布局一致）。
pub fn create_user_task_from_loaded_elf_with_argv(
    loaded : &LoadedElf,
    argv : &[&str],
    envp : &[&str])
    -> Result<(task::TaskId, Vec<u8>), PrepareUserStackError> {
    let prepared = mm::kernel_mm::prepare_elf_user_stack(loaded, argv, envp)?;
    let sp = prepared.sp;
    let (argc, argv_ptr, envp_ptr) = initial_entry_args(sp, argv.len());
    let spec =
        task::user_task_from_loaded_elf(loaded).with_initial_user_sp(sp)
                                               .with_initial_user_args(argc, argv_ptr, envp_ptr);
    Ok((task::create_user_task(spec), prepared.auxv))
}

/// 根据 `prepare_elf_user_stack` 返回的栈顶，推算 argc/argv/envp 指针（与
/// `execve` 用户栈布局一致）。
fn initial_entry_args(sp : usize, argc : usize) -> (usize, usize, usize) {
    let word = core::mem::size_of::<usize>();
    let argv = sp + word;
    let envp = argv + (argc + 1) * word;
    (argc, argv, envp)
}

/// 串行执行单个 ELF 或 shebang 脚本：装载 → spawn → wait → reap；失败或跳过仅 `warn`。
pub fn run_one_elf_argv(log_tag : &str, elf_path : &str, argv : &[&str]) {
    let _ = run_one_elf_argv_exit(log_tag, elf_path, argv);
}

/// 串行执行单个 ELF/脚本并返回退出码；装载/创建失败返回 `None`。
pub fn run_one_elf_argv_exit(log_tag : &str, elf_path : &str, argv : &[&str]) -> Option<isize> {
    let envp = libc_envp_for_path(elf_path);
    run_one_elf_argv_env_exit(log_tag, elf_path, argv, &envp)
}

/// 使用显式初始环境执行一个程序；operator 模式使用此入口，
/// 不根据比赛镜像路径做启发式判断。
pub fn run_one_elf_argv_env_exit(log_tag : &str,
                                 elf_path : &str,
                                 argv : &[&str],
                                 envp : &[&str])
                                 -> Option<isize> {
    // 以 ELF 装载器结果为准；部分根文件系统后端不实现 `exists`，
    // 但实际打开并装载文件仍然可行。
    let load_result = load_program_without_timer_preemption(elf_path, argv);
    let loaded_program = match load_result {
        Ok(program) => program,
        Err(e) => {
            warn!("[{log_tag}] skip load path={elf_path}: {e:?}");
            return None;
        }
    };
    let loaded = loaded_program.elf;
    let final_argv = loaded_program.argv;
    let executable_path = loaded_program.executable_path;

    let final_argv_refs : Vec<&str> = final_argv.iter()
                                                .map(String::as_str)
                                                .collect();
    info!("[{log_tag}] spawn path={elf_path} entry_pc={:#x} satp={:#x} argv={final_argv:?}",
          loaded.entry_pc, loaded.satp);
    let (tid, auxv) =
        match create_user_task_from_loaded_elf_with_argv(&loaded, &final_argv_refs, envp) {
            Ok(created) => created,
            Err(e) => {
                warn!("[{log_tag}] skip spawn path={elf_path}: {e:?}");
                mm::kernel_mm::drop_user_aspace(loaded.user_aspace_ptr);
                return None;
            }
        };

    cred::on_user_task_spawned(tid);

    #[cfg(feature = "vfs-bridge")]
    vfs::cwd::on_user_task_spawned_for_elf(tid,
                                           executable_path.as_str(),
                                           &final_argv_refs);
    #[cfg(feature = "vfs-bridge")]
    let _ = vfs::cwd::set_task_env(tid, envp.iter().copied());
    #[cfg(feature = "vfs-bridge")]
    let _ = vfs::cwd::set_task_auxv(tid, auxv);
    #[cfg(feature = "vfs-bridge")]
    if envp.iter()
           .any(|entry| *entry == "PWD=/root")
    {
        if let Err(error) = vfs::cwd::set_task_cwd(tid, "/root") {
            warn!("[{log_tag}] failed to set operator cwd to /root: {error:?}");
        }
    }
    #[cfg(feature = "vfs-bridge")]
    vfs::mount_ns::on_user_task_spawned(tid);

    #[cfg(feature = "vfs-bridge")]
    if tty::mode() == tty::ConsoleTtyMode::Interactive {
        if let Some(process) = task::process_task_snapshot(tid) {
            tty::set_foreground_pgid(process.pid.raw());
            if tty::controlling_sid() == 0 {
                tty::set_controlling_sid(process.pid.raw());
            }
        }
    }

    let process_pid = task::process_task_snapshot(tid).map(|snapshot| snapshot.pid);
    task::start_user_task(tid);
    task::wait_for_task_exit(tid);
    let exited_process = process_pid.and_then(|pid| wait_and_reap_user_process(pid));
    let exit_code = if let Some(exited_tasks) = exited_process {
        let exit_code = exited_tasks.iter()
                                    .find(|exited| exited.id == tid)
                                    .or_else(|| exited_tasks.first())
                                    .map(|exited| exited.exit_code)
                                    .unwrap_or(-1);
        for exited in &exited_tasks {
            drop_reaped_task_runtime_resources(exited);
        }
        exit_code
    } else {
        // 对损坏的进程 registry 保留仅清理 leader 的旧回退路径；
        // 下方 purge_all_user_processes 仍是有界清理的最后保障。
        task::reap_exited_task(tid).map(|exited| {
                                       drop_reaped_task_runtime_resources(&exited);
                                       exited.exit_code
                                   })
                                   .unwrap_or(-1)
    };

    let (purge, stray_exited) = task::purge_all_user_processes();
    for exited in &stray_exited {
        drop_reaped_task_runtime_resources(exited);
    }
    if purge.killed_tasks > 0 {
        warn!("[{log_tag}] script cleanup killed {} stray user task(s) after path={elf_path}",
              purge.killed_tasks);
    }
    trace!("[{log_tag}] script cleanup summary killed={} reaped={} after path={elf_path}",
           purge.killed_tasks,
           purge.reaped_processes);
    if purge.reaped_processes > 0 {
        trace!("[{log_tag}] script cleanup reaped {} exited process(es) after path={elf_path}",
               purge.reaped_processes);
    }

    syscall::log_thread_bringup_stats_summary();

    trace!("[{log_tag}] END path={elf_path} exit_code={exit_code}");
    Some(exit_code)
}

/// 调度器可能先发布 leader 退出，而远端同组线程尚未完成被中断 syscall 的展开。
/// 按 Linux wait 语义，整个线程组完成后进程才可观察为退出；因此给 exit_group
/// 同组线程一个有界机会到达 `TaskState::Exited`，再原子回收进程，调用方仍保留强制清理回退。
fn wait_and_reap_user_process(pid : task::ProcessId) -> Option<Vec<task::ExitedTask>> {
    let deadline = task::current_tick().saturating_add(PROCESS_EXIT_GRACE_TICKS);
    loop {
        if let Some(exited) = task::reap_exited_process(pid) {
            return Some(exited);
        }
        if task::process_snapshot(pid).is_none() {
            return None;
        }
        let now = task::current_tick();
        if now >= deadline {
            warn!("[user-runner] timed out waiting for process thread group pid={} grace_ticks={}",
                  pid.raw(),
                  PROCESS_EXIT_GRACE_TICKS);
            return None;
        }
        let pending = task::task_ids_for_process(pid).and_then(|task_ids| {
                                                         task_ids.into_iter()
                    .find(|task_id| {
                        !matches!(task::task_state(*task_id),
                                  Some(task::TaskState::Exited(_)))
                    })
                                                     });
        if let Some(task_id) = pending {
            let remaining = deadline.saturating_sub(now)
                                    .max(1);
            let _ = task::wait_for_task_exit_for_ticks(task_id, remaining);
        } else {
            // registry 状态可能在极短窗口内落后于 scheduler。
            task::yield_now();
        }
    }
}

/// 回收已退出任务的用户地址空间与 syscall 侧挂接资源。
fn drop_reaped_task_runtime_resources(exited : &task::ExitedTask) {
    let aspace = exited.trap_frame
                       .as_ref()
                       .map(|frame| frame.user_aspace_ptr())
                       .unwrap_or(0);
    syscall::drop_reaped_task_runtime_resources(exited.id, aspace);
}


/// 按 ELF 路径前缀选择 glibc/musl 的 `LD_LIBRARY_PATH` 与 `PATH`。
fn libc_envp_for_path(path : &str) -> Vec<&'static str> {
    #[cfg(feature = "final_online")]
    {
        let _ = path;
        return vec!["PATH=/glibc:/bin:/usr/bin:/sbin:/usr/sbin"];
    }

    #[cfg(not(feature = "final_online"))]
    if path.starts_with("/glibc/") {
        // LTP 脚本用 `. test.sh` 相对 PATH 加载库；须先于 /glibc/test.sh（lua 包装），
        // 否则 tst_resm 等会落到 PATH 里的 C 二进制并 fork/wait，attach 段与后台 job 组合会卡死。
        vec!["LD_LIBRARY_PATH=/glibc/lib",
             "PATH=/glibc/ltp/testcases/bin:/glibc/ltp/testcases/lib:/glibc:/bin:/usr/bin:/sbin:/\
              usr/sbin"]
    } else if path.starts_with("/musl/") {
        vec!["LD_LIBRARY_PATH=/musl/lib",
             "PATH=/musl/ltp/testcases/bin:/musl/ltp/testcases/lib:/musl:/bin:/usr/bin:/sbin:/usr/\
              sbin"]
    } else {
        Vec::new()
    }
}


/// 串行执行 shell 脚本：等价于 `busybox sh <脚本>`（见 [`BringupCommand`] 自行写 timeout 等）。

/// 装载 ELF 期间屏蔽全局中断，避免定时器抢占打断页表/地址空间临界区。
fn load_program_without_timer_preemption(path : &str,
                                         argv : &[&str])
                                         -> Result<LoadedProgram, LoadProgramError> {
    let state = platform::interrupt::read_global_interrupt_state().ok();
    let _ = platform::interrupt::disable_global_interrupt();
    let result = mm::kernel_mm::load_program_from_path(path, argv);
    if let Some(state) = state {
        let _ = platform::interrupt::restore_global_interrupt_state(state);
    }
    result
}
