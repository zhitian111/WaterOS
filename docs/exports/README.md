# WaterOS 导出文档索引

本目录汇总从 `os/components/**` 导出的组件事实，供协作与评审使用。内容以 `os/Cargo.toml`、`os/feature-tree.txt` 与各组件聚合 `src/lib.rs` 为准，不替代源码 rustdoc。

**版本**：根 crate `wateros` 0.1.0（bring-up 阶段，非发布版）。

## 目录结构

| 子目录 | 内容 | 何时查阅 |
|--------|------|----------|
| [`features/`](features/) | 各一级组件**已实现能力**、feature 矩阵、已知缺口 | 评估组件成熟度、写 roadmap |
| [`public-api/`](public-api/) | 根内核**实际使用**的聚合层导出符号 | 接线、syscall、bring-up 改代码 |
| [`impl-guide/`](impl-guide/) | 新增 api/impl 的步骤与 trait 契约 | 加驱动、FS、VFS、IPC 后端 |
| [`architecture/`](architecture/) | 组件关系总图、api/impl 接线 | 理解全局依赖与 feature 链 |
| [`snapshot/`](snapshot/) | 从各组件导出归纳的**系统快照** | 快速了解当前能跑什么 |
| [`release-overview/`](release-overview/) | 阶段版本的自然语言概述 | 对外说明、里程碑对齐 |

## 一级组件与导出文件

根 `wateros` 直接依赖的组件（`os/Cargo.toml`）及对应导出：

| 组件 | features | public-api | impl-guide |
|------|:--------:|:----------:|:----------:|
| wateros-base | [base](features/wateros-base.md) | [api](public-api/wateros-base.md) | — |
| wateros-abi | [abi](features/wateros-abi.md) | [api](public-api/wateros-abi.md) | — |
| wateros-platform | [platform](features/wateros-platform.md) | [api](public-api/wateros-platform.md) | — |
| wateros-runtime | [runtime](features/wateros-runtime.md) | [api](public-api/wateros-runtime.md) | — |
| wateros-mm | [mm](features/wateros-mm.md) | [api](public-api/wateros-mm.md) | — |
| wateros-task | [task](features/wateros-task.md) | [api](public-api/wateros-task.md) | — |
| wateros-syscall | [syscall](features/wateros-syscall.md) | [api](public-api/wateros-syscall.md) | — |
| wateros-vfs | [vfs](features/wateros-vfs.md) | [api](public-api/wateros-vfs.md) | [guide](impl-guide/wateros-vfs.md) |
| wateros-fs | [fs](features/wateros-fs.md) | [api](public-api/wateros-fs.md) | [guide](impl-guide/wateros-fs.md) |
| wateros-driver | [driver](features/wateros-driver.md) | [api](public-api/wateros-driver.md) | [guide](impl-guide/wateros-driver.md) |
| wateros-ipc | [ipc](features/wateros-ipc.md) | [api](public-api/wateros-ipc.md) | [guide](impl-guide/wateros-ipc.md) |
| wateros-cred | [cred](features/wateros-cred.md) | [api](public-api/wateros-cred.md) | — |
| wateros-klog | [klog](features/wateros-klog.md) | [api](public-api/wateros-klog.md) | — |
| wateros-utils | [utils](features/wateros-utils.md) | [api](public-api/wateros-utils.md) | — |

另有 `wateros-pseudo-shell`（根 feature `pseudo-shell`），尚无独立导出。

## 怎么用

1. **改某个组件前**：先读 `features/<组件>.md` 看现状与缺口，再读 `public-api/<组件>.md` 确认对外符号。
2. **加新 impl**：读 `impl-guide/`（目前覆盖 driver、fs、vfs、ipc）；其余组件参考同目录 `features` 中的 feature 矩阵与 `os/feature-tree.txt`。
3. **看全局**：[`architecture/components.md`](architecture/components.md)（关系图）、[`architecture/module-relations.md`](architecture/module-relations.md)（接线）、[`snapshot/current.md`](snapshot/current.md)（能跑什么）。
4. **对外说明阶段目标**：[`release-overview/current.md`](release-overview/current.md)。

## 与旧文档的关系

- 设计基线、协作指南仍在 `docs/architecture/`、`docs/guides/`、`docs/roadmap/`。
- `docs/architecture/snapshot.md` 是历史主入口；**组件级细节以本目录 `exports/` 为准**，快照摘要见 [`snapshot/current.md`](snapshot/current.md)。

## 维护

导出任务说明见 `docs/tasks/export_*.md`。组件或 feature 变更后，应同步更新对应 `features/`、`public-api/` 条目，并视情况刷新本页所列的全局索引。
