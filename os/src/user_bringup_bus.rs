//! RISC-V64 QEMU OpenSBI 路径上的用户态 bring-up 总线：在 `kernel_main` 中于
//! `fs::init` 之后、`self_tests::task::spawn_all` 与 `fs::test` 的 RW
//! 烟测之前按固定顺序 聚合各里程碑入口。约定见
//! `docs/roadmap/riscv64-busybox/wp-init-test-bus.md`。
//!
//! 当前登记：`stage-00-bus`（总线占位）、
//! `stage-02-mm`（[`crate::user_bringup_mm::run_stage_02`]：
//! MM 子集）、`stage-03-basic`（[`crate::user_bringup_basic::run_stage_03`]：
//! 其余 `/glibc/basic/`、`/musl/basic/` 测程）。 策略与 `fs::test` 一致（warn
//! 后继续）。依赖根卷一致视图的用户 ELF 校验须保持在本总线 内且位于 `fs::test`
//! 的 RW 写盘段之前。

use runtime::logging::*;

/// 运行已登记的 bring-up 阶段（按编号递增顺序）。
///
/// 仅在 `driver::active_impl::init_after_boot` 成功且已执行 `fs::init`
/// 之后调用。
pub fn run() {
    info!("[bringup][stage-00-bus] BEGIN");
    info!("[bringup][stage-00-bus] END");
    // crate::user_bringup_mm::run_stage_02();
    crate::user_bringup_basic::run_stage_03();
}
