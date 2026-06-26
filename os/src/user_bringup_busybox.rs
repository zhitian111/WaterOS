//! `stage-busybox`：挂载根卷后登记内核 runner，串行执行根卷内带 shebang 的测试脚本。

use runtime::logging::*;

const SCRIPT_PATHS : &[&str] = &["/glibc/libcbench_testcode.sh"];

const LOG_TAG : &str = "busybox-bringup";

/// 执行 `stage-busybox`：登记内核串行 runner（不阻塞；用户态在 `run_first_task` 后运行）。
pub fn run_stage_busybox() {
    info!("[bringup][stage-busybox] BEGIN");
    task::spawn_kernel_task(bringup_kernel_runner, 0);
    info!("[{LOG_TAG}] kernel runner enqueued ({} script(s))",
          SCRIPT_PATHS.len());
    info!("[bringup][stage-busybox] END");
}

extern "C" fn bringup_kernel_runner(_arg : usize) -> ! {
    use platform::reset::shutdown;

    crate::user_bringup_root_layout::refresh_ltp_accounts();

    for script_path in SCRIPT_PATHS {
        info!("[{LOG_TAG}] script_path = {script_path}");
        crate::user_bringup_common::run_one_busybox_script(LOG_TAG, script_path);
    }
    info!("[{LOG_TAG}] all scripts finished");
    let _ = shutdown(platform::reset::PlatformResetReason::NoReason);
    task::exit_current(0);
}
