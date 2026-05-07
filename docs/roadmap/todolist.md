# WaterOS TodoList

本文件用于维护阶段性目标、模块开发计划和后续新增任务入口。它不是单次任务记录，而是面向当前阶段目标的持续计划表。

**事实来源**：`os/Cargo.toml`、`os/feature-tree.txt`、各一级组件聚合 `src/lib.rs` 与对应 `Cargo.toml`（截至本版整理时的树状态）。

## 当前阶段目标

- 在 QEMU riscv64 + OpenSBI 路径上持续推进内核 bring-up，并保持默认 feature 链可构建、可自检。
- 在 **`impl-sv39` + `vfs-bridge` + `impl-ext4` + virtio-blk** 等默认组合下，打通 **驱动 → devfs → 根卷挂载 →（可选）VFS 桥接烟囱** 与 **多任务调度 + 最小系统调用分发**。
- 维持 API/impl 分层、feature 选择链和聚合导出链的稳定性；新增 impl 或默认 feature 时同步 `docs/exports/` 与协作文档。
- 持续刷新架构快照、公共 API 与功能说明（按组件拆分，避免单文件过大）。

## 一级组件推进情况

| 组件 | 当前状态 | 下一步重点 |
|------|----------|------------|
| wateros-platform | API/impl/聚合模式稳定；默认 `impl-qemu-riscv64-opensbi` + arch/firmware 子链清晰 | 按需扩展板级与计时器/中断文档；新板级重复同一范式 |
| wateros-driver | 默认路径含 **virtio-mmio 块设备** 与 DTB 扫描；block/character/network 仍为 API + 多数 dummy impl | 充实字符设备与网络栈侧实现；块设备多实例与错误策略 |
| wateros-mm | **`impl-sv39`**、帧分配 **`impl-stack`**、`kernel_mm` / bring-up 相关 API 已在主线使用 | 用户态映射与 `brk` 等与 **syscall** 的真实语义对齐；继续收敛地址空间对外接口 |
| wateros-runtime | console、logging、panic、heap allocator 子 crate 已接入；`pub` 与模块级 rustdoc 已在源码中补齐一轮 | 随子 impl 或默认 feature 变更继续同步 **`docs/exports/features/wateros-runtime.md`** |
| wateros-fs | 默认 **`impl-ext4`**（含 RO/RW 烟测路径）；**devfs/rootfs 的 `impl-kernel`**；与驱动协作完成根块探测与挂载 | 多根设备策略、挂载协议扩展；与 VFS/用户 IO 的边界固化 |
| wateros-vfs | **`impl-fs-bridge`**（feature `bridge-fs-api` / 根上 `vfs-bridge`）将 fs-api 接到 VFS trait；`impl-dummy` 占位仍在 | 路径/会话语义与 fs 侧 RW/RO 视图一致性；减少烟囱式专用 API |
| wateros-ipc | 聚合层导出 **waitqueue**；顶层注释标明 **pipe/signal 等子 crate 尚未接入聚合依赖图**；默认仍为 **impl-dummy** | 将 pipe、signal、futex、shm 等按 feature 接入聚合层并定义 `active_impl` 切换 |
| wateros-task | **`impl-core` + 轮转调度** 已承载可运行路径：`spawn_*`、`yield_now`、`schedule_tick`、等待队列与睡眠等与主线一致 | 与用户态镜像、trap 返回、资源回收相关的边界测试与文档 |
| wateros-abi | **`api-v0`** 与 **`impl-linux-riscv64`** 默认启用；errno、调用号表、参数与 `UserRet` 已供 syscall 使用 | 调用号与内核实际支持集合对齐；版本化 ABI 文档 |
| wateros-syscall | 独立一级 crate，根依赖 **`use syscall as _`** 链接分发符号；当前分发 **yield / exit / write(1,2) / brk 桩** | 扩展 syscall 表、与 MM/VFS 真实能力对接；弱化或替换 `brk` 桩语义 |
| wateros-base | 基础类型与 **base-config**（含 MM 相关常量等） | 避免向上层泄漏板级魔法数；配置与平台边界清晰化 |
| wateros-utils | 通用轻量工具 | 保持无跨层耦合 |

## 当前优先任务

- **IPC**：把子目录 crate（pipe、signal 等）按架构接入 `wateros-ipc` 聚合层，替换「仅 waitqueue + dummy」的过渡状态说明。
- **syscall / ABI**：在保持 `__wateros_syscall_dispatch_current` 稳定的前提下，扩展系统调用集合并与 task/mm/fs 对齐。
- **文档与导出**：默认 feature 变更（如 ext4、vfs-bridge、virtio）已反映到 `docs/exports/` 与 **`docs/architecture/snapshot.md`** 时，继续按组件维护 `public-api`、`impl-guide`、`features`；与对外 API 相关的 **`///`** 变更应同时反映到导出文档或功能快照。
- **新增 impl**：同步 **`docs/guides/task-board.md`** 与路线图本节表格。

## 后续阶段占位（待拆分）

以下条目用于承接跨组件或尚未立项的大块工作，在具体任务文件中拆分为可评审步骤：

- 用户态完整 libc/运行时与内核 syscall 表的联合验证（`user/` 与内核自检协同）。
- 安全与权限模型（能力、命名空间等）仅在设计文档层预留，不默认进入代码路径。

## 新增任务入口

新增任务时请至少补充以下信息：

- 目标组件（可写到上表「下一步」或单独 issue/任务 md）
- 任务类型：设计、实现、文档、重构、验证
- 是否依赖某个 `api-v0`
- 是否需要新增 `impl-*` 或根/组件 feature
- 预计同步更新：`docs/roadmap/todolist.md`、`docs/architecture/snapshot.md`、`docs/exports/`、`docs/guides/` 中的哪些路径
