//! 用户态 bring-up 各阶段共享的 ELF 装载、spawn 与串行等待逻辑。

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

use mm::api::kernel_bringup::{LoadedElf, PrepareUserStackError};
use runtime::logging::*;

/// glibc / musl 根卷前缀。
pub const LIBC_PREFIXES : &[&str] = &["/glibc", "/musl"];

/// 基于已装载 ELF 创建用户任务，并在用户栈上写入 `argv` / `envp`（与 `execve`
/// 布局一致）。
pub fn spawn_user_task_from_loaded_elf_with_argv(loaded : &LoadedElf,
                                                 argv : &[&str],
                                                 envp : &[&str])
                                                 -> Result<task::TaskId, PrepareUserStackError> {
    let sp = mm::kernel_mm::prepare_elf_user_stack(loaded, argv, envp)?;
    let (argc, argv_ptr, envp_ptr) = initial_entry_args(sp, argv.len());
    let spec = task::user_task_from_loaded_elf(loaded)
        .with_initial_user_sp(sp)
        .with_initial_user_args(argc, argv_ptr, envp_ptr);
    Ok(task::spawn_user_task(spec))
}

fn initial_entry_args(sp : usize, argc : usize) -> (usize, usize, usize) {
    let word = core::mem::size_of::<usize>();
    let argv = sp + word;
    let envp = argv + (argc + 1) * word;
    (argc, argv, envp)
}

/// 串行执行单个 ELF：装载 → spawn → wait → reap；失败或跳过仅 `warn`。
pub fn run_one_elf_argv(log_tag : &str, elf_path : &str, argv : &[&str]) {
    let _ = run_one_elf_argv_exit(log_tag, elf_path, argv);
}

/// 串行执行单个 ELF 并返回退出码；装载/创建失败返回 `None`。
pub fn run_one_elf_argv_exit(log_tag : &str, elf_path : &str, argv : &[&str]) -> Option<isize> {
    #[cfg(feature = "vfs-bridge")]
    if let Err(e) = warn_if_elf_missing(log_tag, elf_path) {
        warn!("[{log_tag}] skip path={elf_path}: rootfs check: {e:?}");
        return None;
    }

    let loaded_result = load_elf_without_timer_preemption(elf_path);
    let loaded = match loaded_result {
        Ok(l) => l,
        Err(e) => {
            warn!("[{log_tag}] skip load path={elf_path}: {e:?}");
            return None;
        }
    };

    info!("[{log_tag}] ARGV path={elf_path} argv={argv:?}");

    let tid = match spawn_user_task_from_loaded_elf_with_argv(&loaded, argv, &[]) {
        Ok(t) => t,
        Err(e) => {
            warn!("[{log_tag}] skip spawn path={elf_path}: {e:?}");
            mm::kernel_mm::drop_user_aspace(loaded.user_aspace_ptr);
            return None;
        }
    };

    #[cfg(any(feature = "impl-sv39", feature = "impl-loongarch64"))]
    cred::on_user_task_spawned(tid);

    #[cfg(feature = "vfs-bridge")]
    vfs::cwd::on_user_task_spawned_for_elf(tid, elf_path);

    info!("[{log_tag}] START path={elf_path} tid={tid}");

    task::wait_for_task_exit(tid);
    let exit_code = task::reap_exited_task(tid).map(|e| {
                                                   cred::drop_task_cred(e.id);
                                                   #[cfg(feature = "vfs-bridge")]
                                                   {
                                                       vfs::cwd::drop_task_cwd(e.id);
                                                       vfs::fd::drop_task_fd_table(e.id);
                                                   }
                                                   e.exit_code
                                               })
                                               .unwrap_or(-1);

    info!("[{log_tag}] END path={elf_path} exit_code={exit_code}");
    Some(exit_code)
}

/// 串行执行 `/{prefix}/basic/{name}`，argv 仅含 ELF 自身路径。
pub fn run_one_basic_elf(log_tag : &str, prefix : &str, name : &str) {
    let elf_path = alloc::format!("{prefix}/basic/{name}");
    let argv : Vec<&str> = vec![elf_path.as_str()];
    run_one_elf_argv(log_tag, elf_path.as_str(), &argv);
}

/// 串行执行 busybox + shell 脚本；`script_path` 为根卷内完整路径（如
/// `/glibc/basic_testcode.sh`），busybox 取同目录下的 `busybox`。
pub fn run_one_busybox_script(log_tag : &str, script_path : &str) {
    let busybox_path = match script_path.rfind('/') {
        Some(i) => alloc::format!("{}/busybox", &script_path[..i]),
        None => {
            warn!("[{log_tag}] skip script={script_path}: not an absolute path");
            return;
        }
    };

    #[cfg(feature = "vfs-bridge")]
    if let Err(e) = warn_if_busybox_assets_missing(log_tag,
                                                   busybox_path.as_str(),
                                                   script_path)
    {
        warn!("[{log_tag}] skip script={script_path}: rootfs check: {e:?}");
        return;
    }

    let argv : Vec<&str> = vec![busybox_path.as_str(),
                                "sh",
                                script_path];
    run_one_elf_argv(log_tag, busybox_path.as_str(), &argv);
}

/// 串行执行 BusyBox applet，供 bring-up 分阶段探针使用。
pub fn run_one_busybox_argv(log_tag : &str,
                            busybox_path : &str,
                            argv : &[&str])
                            -> Option<isize> {
    #[cfg(feature = "vfs-bridge")]
    if let Err(e) = warn_if_elf_missing(log_tag, busybox_path) {
        warn!("[{log_tag}] skip busybox argv={argv:?}: rootfs check: {e:?}");
        return None;
    }
    run_one_elf_argv_exit(log_tag, busybox_path, argv)
}

#[cfg(any(feature = "impl-sv39", feature = "impl-loongarch64"))]
fn load_elf_without_timer_preemption(
    elf_path : &str,
) -> Result<LoadedElf, mm::kernel_mm::LoadElfError> {
    let state = platform::interrupt::read_global_interrupt_state().ok();
    let _ = platform::interrupt::disable_global_interrupt();
    let result = mm::kernel_mm::from_elf_path(elf_path);
    if let Some(state) = state {
        let _ = platform::interrupt::restore_global_interrupt_state(state);
    }
    result
}

#[cfg(not(any(feature = "impl-sv39", feature = "impl-loongarch64")))]
fn load_elf_without_timer_preemption(
    elf_path : &str,
) -> Result<LoadedElf, mm::kernel_mm::LoadElfError> {
    mm::kernel_mm::from_elf_path(elf_path)
}

#[cfg(all(any(feature = "impl-sv39", feature = "impl-loongarch64"),
          feature = "vfs-bridge"))]
fn warn_if_elf_missing(log_tag : &str, elf_path : &str) -> Result<(), vfs::api::VfsError> {
    use vfs::api::SingleRootReadView;
    let view = vfs::root::read_view();
    if !view.exists(elf_path)? {
        warn!("[{log_tag}] MISSING elf: {elf_path}");
        return Err(vfs::api::VfsError::NotFound);
    }
    Ok(())
}

#[cfg(all(any(feature = "impl-sv39", feature = "impl-loongarch64"),
          feature = "vfs-bridge"))]
fn warn_if_busybox_assets_missing(log_tag : &str,
                                  busybox_path : &str,
                                  script_path : &str)
                                  -> Result<(), vfs::api::VfsError> {
    use vfs::api::SingleRootReadView;
    let view = vfs::root::read_view();
    if !view.exists(busybox_path)? {
        warn!("[{log_tag}] MISSING busybox: {busybox_path}");
        return Err(vfs::api::VfsError::NotFound);
    }
    if !view.exists(script_path)? {
        warn!("[{log_tag}] MISSING script: {script_path}");
        return Err(vfs::api::VfsError::NotFound);
    }
    Ok(())
}
