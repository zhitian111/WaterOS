//! 用户态 bring-up 各阶段共享的 ELF 装载、spawn 与串行等待逻辑。

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use mm::api::kernel_bringup::{LoadProgramError, LoadedElf, PrepareUserStackError};
use runtime::logging::*;

/// glibc / musl 根卷前缀。
pub const LIBC_PREFIXES : &[&str] = &["/glibc",
//"/musl"
];

/// 基于已装载 ELF 创建用户任务，并在用户栈上写入 `argv` / `envp`（与 `execve`
/// 布局一致）。
pub fn spawn_user_task_from_loaded_elf_with_argv(loaded : &LoadedElf,
                                                 argv : &[&str],
                                                 envp : &[&str])
                                                 -> Result<task::TaskId, PrepareUserStackError> {
    let sp = mm::kernel_mm::prepare_elf_user_stack(loaded, argv, envp)?;
    let (argc, argv_ptr, envp_ptr) = initial_entry_args(sp, argv.len());
    let spec =
        task::user_task_from_loaded_elf(loaded).with_initial_user_sp(sp)
                                               .with_initial_user_args(argc, argv_ptr, envp_ptr);
    Ok(task::spawn_user_task(spec))
}

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
    #[cfg(feature = "vfs-bridge")]
    if let Err(e) = warn_if_path_missing(log_tag, elf_path) {
        warn!("[{log_tag}] skip path={elf_path}: rootfs check: {e:?}");
        return None;
    }

    let load_result = load_program_without_timer_preemption(elf_path, argv);
    let (loaded, final_argv) = match load_result {
        Ok(pair) => pair,
        Err(e) => {
            warn!("[{log_tag}] skip load path={elf_path}: {e:?}");
            return None;
        }
    };

    let final_argv_refs : Vec<&str> = final_argv.iter()
                                                .map(String::as_str)
                                                .collect();
    info!("[{log_tag}] spawn path={elf_path} entry_pc={:#x} satp={:#x} argv={final_argv:?}",
          loaded.entry_pc, loaded.satp);
    let envp = libc_envp_for_path(elf_path);
    let tid = match spawn_user_task_from_loaded_elf_with_argv(&loaded, &final_argv_refs, &envp) {
        Ok(t) => t,
        Err(e) => {
            warn!("[{log_tag}] skip spawn path={elf_path}: {e:?}");
            mm::kernel_mm::drop_user_aspace(loaded.user_aspace_ptr);
            return None;
        }
    };

    cred::on_user_task_spawned(tid);

    #[cfg(feature = "vfs-bridge")]
    vfs::cwd::on_user_task_spawned_for_elf(tid, elf_path, &final_argv_refs);

    task::wait_for_task_exit(tid);
    let exit_code = task::reap_exited_task(tid).map(|e| {
                                                   drop_reaped_task_runtime_resources(&e);
                                                   e.exit_code
                                               })
                                               .unwrap_or(-1);

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

    // 用户进程全部回收后，刷回并丢弃文件页缓存：避免跨脚本累积导致 files/LRU 饱和，
    // 进而引发历史页驱逐时对 ext4 的越界写，以及内核堆持续增长最终卡死。
    #[cfg(feature = "vfs-bridge")]
    if let Err(e) = vfs::reset_file_page_cache() {
        warn!("[{log_tag}] reset file page cache failed after path={elf_path}: {e:?}");
    }

    trace!("[{log_tag}] END path={elf_path} exit_code={exit_code}");
    Some(exit_code)
}

fn drop_reaped_task_runtime_resources(exited : &task::ExitedTask) {
    let aspace = exited.trap_frame
                       .as_ref()
                       .map(|frame| frame.user_aspace_ptr())
                       .unwrap_or(0);
    syscall::drop_reaped_task_runtime_resources(exited.id, aspace);
}


fn libc_envp_for_path(path : &str) -> Vec<&'static str> {
    if path.starts_with("/glibc/") {
        vec!["LD_LIBRARY_PATH=/glibc/lib",
             "PATH=/glibc:/bin:/usr/bin:/sbin:/usr/sbin"]
    } else if path.starts_with("/musl/") {
        vec!["LD_LIBRARY_PATH=/musl/lib",
             "PATH=/musl:/bin:/usr/bin:/sbin:/usr/sbin"]
    } else {
        Vec::new()
    }
}

/// 串行执行 `/{prefix}/basic/{name}`，argv 仅含 ELF 自身路径。
pub fn run_one_basic_elf(log_tag : &str, prefix : &str, name : &str) {
    let elf_path = alloc::format!("{prefix}/basic/{name}");
    let argv : Vec<&str> = vec![elf_path.as_str()];
    run_one_elf_argv(log_tag, elf_path.as_str(), &argv);
}

/// 串行执行 shell 脚本：直接装载同 libc 下的 busybox，argv 为 `sh <脚本>`。
pub fn run_one_busybox_script(log_tag : &str, script_path : &str) {
    let busybox_path = match mm::api::executable::busybox_path_for_script(script_path) {
        Some(path) => path,
        None => {
            warn!("[{log_tag}] skip script={script_path}: unknown libc prefix");
            return;
        }
    };

    #[cfg(feature = "vfs-bridge")]
    if let Err(e) = warn_if_path_missing(log_tag, script_path) {
        warn!("[{log_tag}] skip script={script_path}: rootfs check: {e:?}");
        return;
    }

    let argv : Vec<&str> = vec!["sh",
                                script_path];
    run_one_elf_argv(log_tag, busybox_path, &argv);
}

fn load_program_without_timer_preemption(path : &str,
                                         argv : &[&str])
                                         -> Result<(LoadedElf, Vec<String>), LoadProgramError> {
    let state = platform::interrupt::read_global_interrupt_state().ok();
    let _ = platform::interrupt::disable_global_interrupt();
    let result = mm::kernel_mm::load_program_from_path(path, argv);
    if let Some(state) = state {
        let _ = platform::interrupt::restore_global_interrupt_state(state);
    }
    result
}

#[cfg(feature = "vfs-bridge")]
fn warn_if_path_missing(log_tag : &str, path : &str) -> Result<(), vfs::api::VfsError> {
    use vfs::api::SingleRootReadView;
    let view = vfs::root::read_view();
    if !view.exists(path)? {
        warn!("[{log_tag}] MISSING path: {path}");
        return Err(vfs::api::VfsError::NotFound);
    }
    Ok(())
}
