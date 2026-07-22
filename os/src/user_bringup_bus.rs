//! QEMU 用户态 bring-up 总线：在 `kernel_main` 中于 `fs::init` 之后、
//! `self_tests::task::spawn_all` 与 `fs::test` 的 RW 烟测之前按固定顺序聚合各里程碑入口。约定见
//! `docs/roadmap/riscv64-busybox/wp-init-test-bus.md`.
//!
//! 不需要的阶段直接在 [`run`] 里注释掉对应行即可。策略与 `fs::test` 一致（warn 后继续）。
//! 用户态 bring-up 须在本总线内、位于 `fs::test` 之前，且依赖已挂载的单一 RW 根卷视图。

use runtime::logging::*;

/// 运行已登记的 bring-up 阶段（按编号递增顺序）。
///
/// 仅在 `driver::active_impl::init_after_boot` 成功且已执行 `fs::init`
/// 之后调用。
pub fn run() {
    info!("[bringup][stage-00-bus] BEGIN");
    match fs::mount_default_root_rw() {
        Ok(()) => info!("[bringup][stage-00-bus] ext4 root mounted (RW)"),
        Err(err) => {
            warn!("[bringup][stage-00-bus] mount root RW failed: {err:?} — skip user ELF stages");
            info!("[bringup][stage-00-bus] END");
            return;
        }
    }
    match vfs::ensure_proc_mount_point() {
        Ok(()) => {}
        Err(err) => warn!("[bringup][stage-00-bus] ensure /proc dir failed: {err:?}"),
    }
    match vfs::mount_procfs_at("/proc") {
        Ok(()) => info!("[bringup][stage-00-bus] procfs mounted at /proc"),
        Err(vfs::api::VfsError::Exists) => info!("[bringup][stage-00-bus] procfs already at /proc"),
        Err(err) => warn!("[bringup][stage-00-bus] mount procfs failed: {err:?}"),
    }
    info!("[bringup][stage-00-bus] END");

    #[cfg(feature = "pre")]
    {
        crate::user_bringup_root_layout::ensure_busybox_path_links();
        crate::user_bringup_root_layout::prune_ltp_excluded_testcases();
    }

    // crate::user_bringup_mm::run_stage_02();
    // crate::user_bringup_posix_fs::run_stage_posix_fs_meta();
    //crate::user_bringup_basic::run_stage_basic();
    crate::user_bringup_busybox::run_stage_busybox();
}
