//! QEMU 用户态 bring-up 总线：在 `kernel_main` 中于统一服务初始化和自检之后，
//! 按固定顺序完成根文件系统与用户态入口发布。约定见
//! `docs/roadmap/riscv64-busybox/wp-init-test-bus.md`.
//!
//! 不需要的阶段直接在 [`run`] 里注释掉对应行即可。统一 `self_test` 已在进入本总线前完成。

use runtime::logging::*;

/// 运行已登记的 bring-up 阶段（按编号递增顺序）。
///
/// 仅在统一服务初始化成功且已完成根文件系统准备后调用。
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
    fs::procfs::active_impl::register_uptime_lookup(|| {
        platform::timer::now_duration().map(|duration| duration.as_nanos())
                                       .unwrap_or(0)
    });
    fs::procfs::active_impl::register_idle_time_lookup(|| {
        u128::from(task::total_idle_ticks())
            .saturating_mul(u128::from(base_config::task::SCHED_TIMER_PERIOD_MS))
            .saturating_mul(1_000_000)
    });
    fs::procfs::active_impl::register_task_timer_slack_lookup(syscall::timer_slack_for_task);
    match vfs::mount_bootstrap_procfs_at("/proc") {
        Ok(()) => info!("[bringup][stage-00-bus] procfs mounted at /proc"),
        Err(vfs::api::VfsError::Exists) => info!("[bringup][stage-00-bus] procfs already at /proc"),
        Err(err) => warn!("[bringup][stage-00-bus] mount procfs failed: {err:?}"),
    }
    match vfs::ensure_sys_mount_point() {
        Ok(()) => {}
        Err(err) => warn!("[bringup][stage-00-bus] ensure /sys dir failed: {err:?}"),
    }
    match vfs::mount_bootstrap_sysfs_at("/sys") {
        Ok(()) => info!("[bringup][stage-00-bus] sysfs mounted at /sys"),
        Err(vfs::api::VfsError::Exists) => info!("[bringup][stage-00-bus] sysfs already at /sys"),
        Err(err) => warn!("[bringup][stage-00-bus] mount sysfs failed: {err:?}"),
    }
    info!("[bringup][stage-00-bus] END");

    #[cfg(feature = "pre")]
    {
        crate::user_bringup_root_layout::ensure_busybox_path_links();
        // The skip list contains thousands of entries. Unlinking both libc
        // trees here serializes boot behind thousands of ext4 transactions;
        // exec-time fast-exit handling already prevents excluded workers from
        // blocking the LTP runner.
    }

    // crate::user_bringup_mm::run_stage_02();
    // crate::user_bringup_posix_fs::run_stage_posix_fs_meta();
    //crate::user_bringup_basic::run_stage_basic();
    // 最后才入队 operator/自动评测任务。SMP 下 AP 会立即消费就绪队列，
    // 因而这里同时也是“启动期全局状态已经稳定”的发布边界。
    crate::user_bringup_busybox::run_stage_busybox();
}
