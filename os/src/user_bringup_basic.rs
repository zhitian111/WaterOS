//! `stage-basic`：挂载根卷后登记内核 runner，直接执行
//! `/{glibc,musl}/basic/clone`，用于绕开 BusyBox/shell 依赖验证
//! `clone + wait` 用户态路径。

extern crate alloc;

use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

use mm::api::kernel_bringup::{LoadedElf, PrepareUserStackError};
use runtime::logging::*;

/// 基于已装载 ELF 创建用户任务，并在用户栈上写入 `argv` / `envp`（与 `execve`
/// 布局一致）。
fn spawn_user_task_from_loaded_elf_with_argv(loaded : &LoadedElf,
                                             argv : &[&str],
                                             envp : &[&str])
                                             -> Result<task::TaskId, PrepareUserStackError> {
    let sp = mm::kernel_mm::prepare_elf_user_stack(loaded, argv, envp)?;
    let spec = task::user_task_from_loaded_elf(loaded).with_initial_user_sp(sp);
    Ok(task::spawn_user_task(spec))
}

const BASIC_TESTS : &[&str] = &["clone"];
const LIBC_PREFIXES : &[&str] = &["/glibc", "/musl"];

/// 执行 `stage-basic`：登记内核串行 runner（不阻塞；用户态在 `run_first_task`
/// 后运行）。
pub fn run_stage_basic() {
    info!("[bringup][stage-basic] BEGIN");
    #[cfg(not(any(feature = "impl-sv39", feature = "impl-loongarch64")))]
    {
        warn!("[basic-bringup] no mm impl: skip");
        info!("[bringup][stage-basic] END");
        return;
    }
    #[cfg(any(feature = "impl-sv39", feature = "impl-loongarch64"))]
    {
        task::spawn_kernel_task(bringup_kernel_runner, 0);
        info!("[basic-bringup] kernel runner enqueued ({} test(s) × {} libc)",
              BASIC_TESTS.len(),
              LIBC_PREFIXES.len());
    }
    info!("[bringup][stage-basic] END");
}

#[cfg(any(feature = "impl-sv39", feature = "impl-loongarch64"))]
extern "C" fn bringup_kernel_runner(_arg : usize) -> ! {
    for prefix in LIBC_PREFIXES {
        for test in BASIC_TESTS {
            run_one_basic_elf(prefix, test);
        }
    }
    info!("[basic-bringup] all enabled tests finished");
    task::exit_current(0);
}

#[cfg(any(feature = "impl-sv39", feature = "impl-loongarch64"))]
fn run_one_basic_elf(prefix : &str, test : &str) {
    let elf_path = format!("{prefix}/basic/{test}");

    #[cfg(feature = "vfs-bridge")]
    if let Err(e) = warn_if_missing(elf_path.as_str()) {
        warn!("[basic-bringup] skip path={}: rootfs check: {:?}",
              elf_path, e);
        return;
    }

    let loaded = match mm::kernel_mm::from_elf_path(elf_path.as_str()) {
        Ok(l) => l,
        Err(e) => {
            warn!("[basic-bringup] skip load path={}: {:?}",
                  elf_path, e);
            return;
        }
    };

    let argv : Vec<&str> = vec![elf_path.as_str()];

    let tid = match spawn_user_task_from_loaded_elf_with_argv(&loaded, &argv, &[]) {
        Ok(t) => t,
        Err(e) => {
            warn!("[basic-bringup] skip spawn path={}: {:?}",
                  elf_path, e);
            mm::kernel_mm::drop_user_aspace(loaded.user_aspace_ptr);
            return;
        }
    };

    #[cfg(any(feature = "impl-sv39", feature = "impl-loongarch64"))]
    cred::on_user_task_spawned(tid);

    #[cfg(feature = "vfs-bridge")]
    vfs::cwd::on_user_task_spawned_for_elf(tid, elf_path.as_str());

    info!("[basic-bringup] START path={elf_path} tid={tid}");

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

    info!("[basic-bringup] END path={elf_path} exit_code={exit_code}");
}

#[cfg(all(any(feature = "impl-sv39", feature = "impl-loongarch64"),
          feature = "vfs-bridge"))]
fn warn_if_missing(elf_path : &str) -> Result<(), vfs::api::VfsError> {
    use vfs::api::SingleRootReadView;
    let view = vfs::root::read_view();
    if !view.exists(elf_path)? {
        warn!("[basic-bringup] MISSING elf: {}",
              elf_path);
        return Err(vfs::api::VfsError::NotFound);
    }
    Ok(())
}
