//! `stage-busybox`：挂载根卷后登记内核 runner，串行执行
//! `/{glibc,musl}/busybox sh /{prefix}/*_testcode.sh`。
//!
//! 测例表 [`TESTCASES`] 可逐条设 `enabled: false` 或注释行以跳过。

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

/// 单个赛题脚本组（不含 `_testcode.sh` 后缀）。
struct TestCaseSpec {
    name : &'static str,
    enabled : bool,
}

/// 赛题推荐顺序（见 `docs/roadmap/test-case-full-pass-plan.md`）；LTP 置末。
const TESTCASES : &[TestCaseSpec] =
    &[TestCaseSpec { name : "basic",
                     enabled : true } /* TestCaseSpec { name: "busybox", enabled: true },
                                       * TestCaseSpec { name: "lua", enabled: true },
                                       * TestCaseSpec { name: "libctest", enabled: true },
                                       * TestCaseSpec { name: "iozone", enabled: true },
                                       * TestCaseSpec { name: "unixbench", enabled: true },
                                       * TestCaseSpec { name: "lmbench", enabled: true },
                                       * TestCaseSpec { name: "iperf", enabled: true },
                                       * TestCaseSpec { name: "netperf", enabled: true },
                                       * TestCaseSpec { name: "libcbench", enabled: true },
                                       * TestCaseSpec { name: "cyclictest", enabled: true },
                                       * TestCaseSpec { name: "ltp", enabled: true }, */];

const LIBC_PREFIXES : &[&str] = &["/glibc", "/musl"];

/// 执行 `stage-busybox`：登记内核串行 runner（不阻塞；用户态在 `run_first_task`
/// 后运行）。
pub fn run_stage_busybox() {
    info!("[bringup][stage-busybox] BEGIN");
    #[cfg(not(any(feature = "impl-sv39", feature = "impl-loongarch64")))]
    {
        warn!("[busybox-bringup] no mm impl: skip");
        info!("[bringup][stage-busybox] END");
        return;
    }
    #[cfg(any(feature = "impl-sv39", feature = "impl-loongarch64"))]
    {
        task::spawn_kernel_task(bringup_kernel_runner, 0);
        info!("[busybox-bringup] kernel runner enqueued ({} testcase slot(s) × {} libc)",
              TESTCASES.iter()
                       .filter(|t| t.enabled)
                       .count(),
              LIBC_PREFIXES.len());
    }
    info!("[bringup][stage-busybox] END");
}

#[cfg(any(feature = "impl-sv39", feature = "impl-loongarch64"))]
extern "C" fn bringup_kernel_runner(_arg : usize) -> ! {
    for prefix in LIBC_PREFIXES {
        for tc in TESTCASES {
            if !tc.enabled {
                continue;
            }
            run_one_busybox_script(prefix, tc.name);
        }
    }
    info!("[busybox-bringup] all enabled testcases finished");
    task::exit_current(0);
}

#[cfg(any(feature = "impl-sv39", feature = "impl-loongarch64"))]
fn run_one_busybox_script(prefix : &str, name : &str) {
    let busybox_path = format!("{prefix}/busybox");
    let script_path = format!("{prefix}/{name}_testcode.sh");

    #[cfg(feature = "vfs-bridge")]
    if let Err(e) = warn_if_missing(&busybox_path, &script_path) {
        warn!("[busybox-bringup] skip prefix={prefix} name={name}: rootfs check: {:?}",
              e);
        return;
    }

    let loaded = match mm::kernel_mm::from_elf_path(busybox_path.as_str()) {
        Ok(l) => l,
        Err(e) => {
            warn!("[busybox-bringup] skip load busybox path={}: {:?}",
                  busybox_path, e);
            return;
        }
    };

    let argv : Vec<&str> = vec![busybox_path.as_str(), "sh", script_path.as_str(),];

    let tid = match spawn_user_task_from_loaded_elf_with_argv(&loaded, &argv, &[]) {
        Ok(t) => t,
        Err(e) => {
            warn!("[busybox-bringup] skip spawn path={} script={}: {:?}",
                  busybox_path, script_path, e);
            mm::kernel_mm::drop_user_aspace(loaded.user_aspace_ptr);
            return;
        }
    };

    #[cfg(any(feature = "impl-sv39", feature = "impl-loongarch64"))]
    cred::on_user_task_spawned(tid);

    #[cfg(feature = "vfs-bridge")]
    vfs::cwd::on_user_task_spawned_for_elf(tid, busybox_path.as_str());

    info!("[busybox-bringup] START prefix={prefix} script={script_path} tid={tid}");

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

    info!("[busybox-bringup] END prefix={prefix} script={script_path} exit_code={exit_code}");
}

#[cfg(all(any(feature = "impl-sv39", feature = "impl-loongarch64"),
          feature = "vfs-bridge"))]
fn warn_if_missing(busybox_path : &str, script_path : &str) -> Result<(), vfs::api::VfsError> {
    use vfs::api::SingleRootReadView;
    let view = vfs::root::read_view();
    if !view.exists(busybox_path)? {
        warn!("[busybox-bringup] MISSING busybox: {}",
              busybox_path);
        return Err(vfs::api::VfsError::NotFound);
    }
    if !view.exists(script_path)? {
        warn!("[busybox-bringup] MISSING script: {}",
              script_path);
        return Err(vfs::api::VfsError::NotFound);
    }
    Ok(())
}
