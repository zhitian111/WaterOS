# wateros-ipc 公共 API 快照

## 用途

描述 **`wateros-ipc`** 聚合层当前 **实际接入** 的子 crate：**`ipc-api-v0`**、**`ipc-impl-dummy`**、**`ipc-waitqueue`**。**`ipc-pipe`**、**`ipc-signal`**、**`ipc-shm`** 等目录内 crate 在 **`feature-tree.txt`** 中仍存在，但 **尚未** 进入本聚合 **`Cargo.toml`** 依赖图，故无对应 **`pub mod`**。

## 事实来源

- [`os/components/wateros-ipc/Cargo.toml`](../../os/components/wateros-ipc/Cargo.toml)
- [`os/components/wateros-ipc/src/lib.rs`](../../os/components/wateros-ipc/src/lib.rs)
- [`os/components/wateros-ipc/ipc-waitqueue/src/lib.rs`](../../os/components/wateros-ipc/ipc-waitqueue/src/lib.rs)

## Feature

| Feature | 说明 |
|---------|------|
| **`default`** | `api-v0` + **`impl-dummy`**。 |
| **`impl-dummy`** | 占位名；启用时根层 **`pub use impl_dummy as active_impl`**。 |

若关闭 **`impl-dummy`** feature，**`active_impl`** 别名不再导出，但 **`api`** 与 **`waitqueue`** 仍保留。

## 聚合层导出

| 项 | 说明 |
|----|------|
| **`pub mod api`** | **`pub use ::api_v0::*`**，完整 **`wateros-ipc-api-v0`** 根公共项。 |
| **`active_impl`** | 仅 **`#[cfg(feature = "impl-dummy")]`**：**`impl_dummy`** 模块别名，占位实现命名空间。 |
| **`pub mod waitqueue`** | **`pub use ::waitqueue::*`**：重导出 **`TaskId`**、**`TaskTick`**、**`TaskWaitHandle`**、**`TaskWaitResult`**、**`WaitQueueId`**；**`WaitQueue`** 为对 **`wateros_task::WaitQueue`** 的薄包装（**`new`**、**`id`**、**`wait_handle`**、**`wait_current`**、**`wait_current_for_ticks`**、**`wake_one`**、**`wake_all`**），并实现 **`Default`**。 |

## 缺口说明

- 管道、信号、futex、共享内存等能力未聚合进本 crate；**`active_impl`** 当前仅为 dummy 占位。

## 维护要求

聚合依赖图或 **`lib.rs`** 再导出变化时，同步更新本文件与 **`docs/exports/features/wateros-ipc.md`**。
