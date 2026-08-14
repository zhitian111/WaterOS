//! userspace-init 模式：rootfs 存在 `/sbin/init` 时作为首用户进程启动。
//!
//! 由 `/sbin/init`（busybox init）按 `/etc/inittab` 执行 rcS 并派生 getty/shell；
//! 无 `/sbin/init` 的镜像（比赛 LTP 镜像等）回退到 operator/LTP 队列。

use runtime::logging::*;
use vfs::api::SingleRootReadView;

const LOG_TAG : &str = "bringup-init";
const INIT_PATH : &str = "/sbin/init";

/// 若 rootfs 存在 `/sbin/init`，派生 init 内核任务并返回 `true`；否则返回 `false`。
pub fn try_start_init() -> bool {
    match vfs::root::read_view().exists(INIT_PATH) {
        Ok(true) => {
            info!("[{LOG_TAG}] {INIT_PATH} present; entering userspace-init mode");
            task::spawn_kernel_task(init_main, 0);
            true
        }
        Ok(false) => false,
        Err(error) => {
            warn!("[{LOG_TAG}] cannot probe {INIT_PATH}: {error:?}; fallback");
            false
        }
    }
}

/// 运行 `/sbin/init` 并等待；init 不应退出，若退出则停机等待。
extern "C" fn init_main(_arg : usize) -> ! {
    info!("[{LOG_TAG}] launching {INIT_PATH}");
    crate::user_bringup_common::run_one_elf_argv_exit(LOG_TAG, INIT_PATH, &["init"]);
    error!("[{LOG_TAG}] {INIT_PATH} exited unexpectedly; halting");
    loop {
        platform::arch::interrupt::wait_for_interrupt();
    }
}
