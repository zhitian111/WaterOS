# os 内核二进制 — 已实现功能快照

## 用途

记录根 crate `wateros`（`os/`）作为 **内核可执行文件** 的职责、bring-up 阶段与已知缺口。事实来源：`os/src/**`、`os/Cargo.toml`、`os/build.rs`。

本 crate **不是**可链接库；能力体现在启动顺序、全局 handler 与用户态 bring-up 总线上。

## 子模块与职责

| 模块 | 职责 | 编译条件 |
|------|------|----------|
| `main.rs` | `panic`/`alloc` 委托、`kernel_main`（按 board 分模块） | 始终 |
| `build.rs` | 链接脚本与 `_start.S` 重链声明 | 始终 |
| `boot_timebase` | DTB `timebase-frequency` → `platform::time` | QEMU board |
| `trap_handler` | 组合层 trap/syscall/信号/调度 tick | QEMU board |
| `user_bringup_bus` | bring-up 总线：挂载根卷 + 阶段调度 | QEMU board |
| `user_bringup_common` | ELF 装载、spawn、wait/reap 串行执行 | QEMU board |
| `user_bringup_basic` | `stage-basic`：直接跑 `/{glibc,musl}/basic/*` | QEMU board |
| `user_bringup_busybox` | `stage-busybox`：busybox + testcode.sh 队列 | QEMU board |
| `user_operator` | 按编译期 feature 选择自动队列 / 交互 shell / 指定脚本 | QEMU board |
| `user_bringup_mm` | `stage-02-mm`：并行 spawn MM 测程 | QEMU board |
| `user_bringup_posix_fs` | POSIX 目录/重命名烟囱 | QEMU board |
| `user_bringup_root_layout` | `/bin` 链接、`/etc/passwd`、LTP 账户刷新 | QEMU board |
| `self_tests::network` | 网络栈同步烟测 + 空 `spawn_all` | QEMU board |
| `self_tests::task` | hello/pipe 自检（**当前 `spawn_all` 禁用**） | 仅 `qemu-riscv64-opensbi` |

## Feature 矩阵（根 crate）

| Feature | 效果 |
|---------|------|
| `qemu-riscv64-opensbi`（default） | Sv39 MM、RISC-V virt 平台、完整 bring-up |
| `qemu-loongarch64-virt` | LoongArch 三级页表、关 MMU 后再建页表、无 `self_tests::task` |
| `vfs-bridge` | 启用 `wateros-vfs`；bring-up 路径检查、procfs、CWD |
| `bringup-ltp-glibc-only` / `bringup-ltp-musl-only` | busybox 阶段仅跑单侧 LTP |
| `operator-shell` / `operator-run` | 开发/诊断启动行为；shell 路径与脚本在构建期嵌入 |
| `syscall-trace` | trap 热路径 `trace!`（与 `debug_assertions` 二选一开启） |
| `pseudo-shell` | 依赖 `wateros-pseudo-shell`（需自行调用，非默认 bring-up） |
| `impl-sv39` | 根 flag，联动子 crate 的 Sv39 相关 `cfg` |

## 已实现能力

- **双板 `kernel_main`**：固定顺序初始化 runtime → arch → task/trap → MM → driver → fs → bring-up → 定时器 → `run_first_task`。
- **DTB timebase 探测**：失败回退平台默认频率。
- **组合 trap 路由**：syscall 分派、lazy/COW 页错、定时器调度、信号与 `rt_sigreturn`。
- **网络 poller 内核任务** + 调度前 TCP/UDP 同步烟测。
- **用户 bring-up 总线**：RW ext4 根卷、`/proc`、busybox 硬链接布局、可注释切换的阶段。
- **串行用户测程**：装载时关中断、spawn 后 wait/reap、脚本结束 purge 残留用户进程。
- **全局错误路径**：panic / OOM 委托 runtime。

## 与组件文档的交叉引用

- 启动时序细节：[`kernel-entry.md`](../architecture/kernel-entry.md)
- 各一级组件能力：[`docs/exports/features/`](../features/)

## 缺口与后续

- `self_tests::task::spawn_all` 已整体禁用；用户回归依赖 `stage-busybox`。
- `user_bringup_bus::run` 内多数阶段默认注释，需手动打开 `basic` / `mm` / `posix-fs`。
- LoongArch 路径未调用 `self_tests::task::spawn_all`（模块未编译）。
- 根 crate 无单元测试；验证依赖 QEMU 启动日志与 LTP/赛题脚本。
- `pseudo-shell` feature 未在 `kernel_main` 默认挂接。

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出（注释/inline 任务同步） |
