//! RISC-V64 QEMU OpenSBI 路径上的用户态 bring-up 总线：在 `kernel_main` 中于
//! `fs::init` 之后、`self_tests::task::spawn_all` 与 `fs::test` 的 RW
//! 烟测之前按固定顺序聚合各里程碑入口。约定见
//! `docs/roadmap/riscv64-busybox/wp-init-test-bus.md`.
//!
//! 当前登记：`stage-00-bus`（挂载 ext4 **RW** 根卷）、
//! `stage-busybox`（[`crate::user_bringup_busybox::run_stage_busybox`]：
//! 内核 runner 串行 `busybox sh *_testcode.sh`）。策略与 `fs::test` 一致（warn
//! 后继续）。用户态 bring-up 须在本总线内、位于 `fs::test` 之前，且依赖已挂载的
//! 单一 RW 根卷视图。

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
            warn!(
                "[bringup][stage-00-bus] mount root RW failed: {:?} — skip user ELF stages",
                err
            );
            info!("[bringup][stage-00-bus] END");
            return;
        }
    }
    info!("[bringup][stage-00-bus] END");
    #[cfg(feature = "impl-loongarch64")]
    {
        warn!(
            "[bringup][stage-01-loongarch64] user ELF stages skipped until PGDL/TLB user page \
               switching is enabled"
        );
    }
    #[cfg(not(feature = "impl-loongarch64"))]
    {
        crate::user_bringup_busybox::run_stage_busybox();
    }
}
