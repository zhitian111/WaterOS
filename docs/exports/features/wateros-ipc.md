# wateros-ipc 功能快照

## 用途

记录 **`wateros-ipc`** 聚合 crate 当前实际编入工作区的子包与对外导出，区分「目录已存在」的 **pipe / signal / shm** 等子工程与「已进入默认依赖图」的能力。

## 事实来源

- `os/components/wateros-ipc/Cargo.toml`（**`[workspace].members`** 列表）
- `os/components/wateros-ipc/src/lib.rs`
- `os/components/wateros-ipc/ipc-waitqueue/`
- `os/feature-tree.txt`（子包 feature 树）

## 聚合层当前接线

- **`default`**：`api-v0`、`impl-dummy`。
- **`pub mod api`**：**`ipc-api/api-v0`** 占位面（示例级 API）。
- **`active_impl`**：在 **`impl-dummy`** 下指向 **`impl_dummy`**。
- **`pub mod waitqueue`**：**`ipc-waitqueue`** — 对 **`wateros-task::WaitQueue`** 的薄封装（**`TaskId`**、**`TaskWaitHandle`**、**`WaitQueueId`** 等来自 task）。

## 子目录但未接入聚合默认构建

- **`ipc-pipe`**、**`ipc-signal`**、**`ipc-shm`**、**`ipc-futex`**、**`ipc-event`** 等子 crate 在仓库中存在各自 **`Cargo.toml`** 与占位实现，**未**列入 **`wateros-ipc`** 根 workspace members，**根聚合 **`Cargo.toml`** 亦未依赖它们**；内核 **`os/src`** 当前**无** **`ipc::`** 业务引用（根 **`wateros`** 仍声明 **`ipc`** 依赖，属「可链接、待接线」状态）。

## 明确未覆盖

- pipe、signal、shm、futex、event 等完整 IPC 语义与 syscall / task fd 表打通。
- **`ipc-api`** 从占位演进为稳定契约并实现非 dummy **`impl-*`**。

## 维护要求

将子 crate 纳入 workspace、根依赖或 **`os/src`** 调用时，同步更新本文件、**`docs/architecture/snapshot.md`** 与 **`docs/roadmap/todolist.md`**。
