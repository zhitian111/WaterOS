# WaterOS TodoList

本文件用于维护阶段性目标、模块开发计划和后续新增任务入口。它不是单次任务记录，而是面向当前阶段目标的持续计划表。

**事实来源**：`os/Cargo.toml`、`os/feature-tree.txt`、各一级组件聚合 `src/lib.rs` 与对应 `Cargo.toml`（截至本版整理时的树状态）。

## 当前阶段目标

- 在 QEMU riscv64 + OpenSBI 路径上持续推进内核 bring-up，并保持默认 feature 链可构建、可自检。
- 在 **`impl-sv39` + `vfs-bridge` + `impl-ext4` + virtio-blk** 等默认组合下，打通 **驱动 → devfs → 根卷挂载 →（可选）VFS 桥接烟囱** 与 **多任务调度 + 最小系统调用分发**。
- 在 QEMU loongarch64 virt 路径上维持 boot、trap、timer 与 round-robin kernel task 的可回归 bring-up。
- 维持 API/impl 分层、feature 选择链和聚合导出链的稳定性；新增 impl 或默认 feature 时同步 `docs/exports/` 与协作文档。
- 持续刷新架构快照、公共 API、架构图与功能说明（按组件拆分，避免单文件过大）。

## 一级组件推进情况

| 组件 | 当前状态 | 下一步重点 |
|------|----------|------------|
| wateros-platform | API/impl/聚合模式稳定；默认 `impl-qemu-riscv64-opensbi`；LoongArch64 virt 已具备 UART、时间频率与 CSR timer 初步路径 | 板级与计时器/中断文档；补齐 LoongArch reset、paging 与平台能力说明 |
| wateros-driver | 默认路径含 **virtio-mmio 块设备** 与 DTB 扫描；block/character/network 仍为 API + 多数 dummy impl | 充实字符设备与网络栈侧实现；块设备多实例与错误策略 |
| wateros-mm | **`impl-sv39`**、帧分配 **`impl-stack`**、`kernel_mm` / bring-up 相关 API 已在 RISC-V 主线使用；**用户态 `brk`/`mmap`/`munmap`/`mprotect`** 已与 syscall、ELF 装载字段 `user_aspace_ptr` 联调第一版 | 用户 **`UserMemoryOps`** 与 trap 拷贝全路径；LoongArch 侧避免直接照搬 Sv39 假设 |
| wateros-runtime | console、logging、panic、heap allocator 子 crate 已接入；`pub` 与模块级 rustdoc 已在源码中补齐一轮 | 随子 impl 或默认 feature 变更继续同步 **`docs/exports/features/wateros-runtime.md`** |
| wateros-fs | 默认 **`impl-ext4`**（含 RO/RW 烟测路径）；**devfs/rootfs 的 `impl-kernel`**；与驱动协作完成根块探测与挂载 | 多根设备策略、挂载协议扩展；与 VFS/用户 IO 的边界固化 |
| wateros-vfs | **`impl-fs-bridge`**（feature `bridge-fs-api` / 根上 `vfs-bridge`）将 fs-api 接到 VFS trait；`impl-dummy` 占位仍在 | 路径/会话语义与 fs 侧 RW/RO 视图一致性；减少烟囱式专用 API |
| wateros-ipc | 聚合层导出 **waitqueue**；**pipe** 已通过 feature 接入并具备内核内部 ring-buffer 与 fd endpoint；signal/futex/shm/event 仍为占位或未接入 | pipe fork/dup/close-on-exit 语义；继续接入 signal、futex、shm 等 feature |
| wateros-task | **`impl-core` + 轮转调度**；RISC-V 主线与用户态自检一致；已提供条件等待、最小父子关系与 child-exit 等待服务 IPC/syscall；LoongArch64 上可跑 kernel task 轮转 | trap 驱动抢占、用户任务恢复、block object 抽象、TaskHandle generation 与跨架构文档 |
| wateros-abi | **`api-v0`** 与 **`impl-linux-generic64`**（经 **`impl-linux-riscv64`** 等别名）默认启用；errno、号表、参数与 `UserRet` 已供 syscall 使用 | 调用号与内核实际支持集合对齐；版本化 ABI 文档 |
| wateros-syscall | 独立一级 crate，根依赖 **`use syscall as _`**；RISC-V 主线默认链接 **`wateros-mm`**，在 syscall 层拼合 **`brk`/`mmap`/`munmap`/`mprotect`**；read/write/close/pipe2 经 **`wateros-vfs::fd`** | 扩展 syscall 表、**`openat`** 与 VFS 文件句柄；补 fd 继承/dup/自动关闭；**`UserMemoryOps`** 与 write 安全拷贝 |
| wateros-cred | **设计已定稿**（见 **`docs/guides/cred-module-design.md`**）；代码尚未 scaffold | 实现 `cred-api` + `impl-root`；getuid/euid/gid/egid/getgroups；fork/exec 生命周期；VFS stat 占位 |
| wateros-base | 基础类型与 **base-config**（含 MM 相关常量等） | 避免向上层泄漏板级魔法数；配置与平台边界清晰化 |
| wateros-utils | 通用轻量工具 | 保持无跨层耦合 |

## 当前优先任务

- **IPC**：pipe 已作为内核内部对象接入 `wateros-ipc` 聚合层，并完成最小 fd/syscall smoke；下一步补 fork/dup/任务退出关闭语义，并继续推进 signal、futex、shm 等子模块。
- **syscall / ABI**：在保持 `__wateros_syscall_dispatch_current` 稳定的前提下，扩展系统调用集合并与 task/mm/fs 对齐。
- **文档与导出**：默认 feature 变更（如 ext4、vfs-bridge、virtio）已反映到 `docs/exports/` 与 **`docs/architecture/snapshot.md`** 时，继续按组件维护 `public-api`、`impl-guide`、`features`；与对外 API 相关的 **`///`** 变更应同时反映到导出文档或功能快照。
- **新增 impl**：同步 **`docs/guides/task-board.md`** 与路线图本节表格。

## 赛题 test_case 全通过专项

分阶段路线、测例依赖表与可勾选清单见 **`docs/roadmap/test-case-full-pass-plan.md`**（与 `test_case/README.md`、`docs/prompts` 配合使用）。

## RISC-V64 BusyBox bring-up（并行工作包）

仅 riscv64、按模块拆分的可并行计划、各子任务独立 md、验收与 **`kernel_main` 上 init/test 总线**（不含 `self_tests`）约定见 **`docs/roadmap/riscv64-busybox/README.md`**。总线骨架见 **`os/src/user_bringup_bus.rs`**；**`stage-02-mm`** 从根卷 **`/glibc/basic/`** 装载测程 ELF 见 **`os/src/user_bringup_mm.rs`**；详见 **`docs/roadmap/riscv64-busybox/wp-init-test-bus.md`**。

## 后续阶段占位（待拆分）

以下条目用于承接跨组件或尚未立项的大块工作，在具体任务文件中拆分为可评审步骤：

- 用户态完整 libc/运行时与内核 syscall 表的联合验证（`user/` 与内核自检协同）。
- **进程凭证（`wateros-cred`）**：设计方案见 **`docs/guides/cred-module-design.md`**；首版 MVP 后逐步对接 ext4 inode owner 与 VFS 权限。

## 新增任务入口

新增任务时请至少补充以下信息：

- 目标组件（可写到上表「下一步」或单独 issue/任务 md）
- 任务类型：设计、实现、文档、重构、验证
- 是否依赖某个 `api-v0`
- 是否需要新增 `impl-*` 或根/组件 feature
- 预计同步更新：`docs/roadmap/todolist.md`、`docs/architecture/snapshot.md`、`docs/exports/`、`docs/guides/` 中的哪些路径
