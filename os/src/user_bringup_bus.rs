//! QEMU 用户态 bring-up 总线：在 `kernel_main` 中于统一服务初始化和自检之后，
//! 按固定顺序完成根文件系统与用户态入口发布。约定见
//! `docs/roadmap/riscv64-busybox/wp-init-test-bus.md`.
//!
//! 不需要的阶段直接在 [`run`] 里注释掉对应行即可。统一 `self_test` 已在进入本总线前完成。

use runtime::logging::*;

use alloc::{string::String, vec::Vec};
use core::fmt::Write;

fn sysvipc_table(table : fs::procfs::api::SysVIpcTable) -> Vec<u8> {
    match table {
        fs::procfs::api::SysVIpcTable::Shm => {
            // 先在 SHM 锁内复制稳定快照，格式化期间不持 IPC 全局锁。
            let segments = ipc::shm::registry().lock().segment_infos();
            let mut out = String::from(
                "       key      shmid perms                  size  cpid  lpid nattch   uid   gid  cuid  cgid      atime      dtime      ctime                   rss                  swap\n",
            );
            for segment in segments {
                let mode = segment.mode | if segment.marked_removed { 0o1000 } else { 0 };
                let _ = writeln!(out,
                                 "{:10} {:10}  {:4o} {:21} {:5} {:5} {:6} {:5} {:5} {:5} {:5} {:10} {:10} {:10} {:21} {:21}",
                                 segment.key as i32,
                                 segment.shmid,
                                 mode,
                                 segment.size,
                                 segment.creator_pid,
                                 segment.last_pid,
                                 segment.nattch,
                                 segment.owner_uid,
                                 segment.owner_gid,
                                 segment.creator_uid,
                                 segment.creator_gid,
                                 segment.attach_time,
                                 segment.detach_time,
                                 segment.change_time,
                                 segment.size,
                                 0);
            }
            out.into_bytes()
        }
        fs::procfs::api::SysVIpcTable::Msg => {
            b"       key      msqid perms      cbytes       qnum lspid lrpid   uid   gid  cuid  cgid      stime      rtime      ctime\n".to_vec()
        }
        fs::procfs::api::SysVIpcTable::Sem => {
            b"       key      semid perms      nsems   uid   gid  cuid  cgid      otime      ctime\n".to_vec()
        }
    }
}

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
    fs::procfs::active_impl::register_sysvipc_table_lookup(sysvipc_table);
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
