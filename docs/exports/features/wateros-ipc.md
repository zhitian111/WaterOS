# wateros-ipc 功能快照

## 用途

记录 **`wateros-ipc`** 聚合 crate 当前实际编入工作区的子包与对外导出，区分「目录已存在」的 **signal / shm / event** 等子工程与「已进入默认依赖图」的能力。

## 事实来源

- `os/components/wateros-ipc/Cargo.toml`
- `os/components/wateros-ipc/src/lib.rs`
- `os/components/wateros-ipc/ipc-waitqueue/`
- `os/components/wateros-ipc/ipc-pipe/`
- `os/components/wateros-ipc/ipc-futex/`
- `os/feature-tree.txt`（子包 feature 树）

## 聚合层当前接线

- **`default`**：`api-v0`、`impl-dummy`。
- **`impl-riscv64` / `impl-loongarch64`**：`waitqueue/impl-*`；可选 **`futex/impl-task`**（`futex` feature 启用时）。
- **`pub mod api`**：**`ipc-api/api-v0`** 占位面（示例级 API）。
- **`active_impl`**：在 **`impl-dummy`** 下指向 **`impl_dummy`**。
- **`pub mod waitqueue`**：**`ipc-waitqueue`** — 对 **`wateros-task::WaitQueue`** 的薄封装。
- **`pipe` feature**：接入 **`ipc-pipe`**（`pipe-api/api-v0` + `pipe-impl/impl-ringbuf`）。
- **`futex` feature**：接入 **`ipc-futex`**（`futex-api/api-v0` + **`futex-impl/impl-task`** 默认）。导出 **`FutexHub`**、**`KernelFutexOps`**、**`FutexKey`**、robust 布局常量；syscall 经 **`ipc::futex::FutexHub::global()`** 委托 wait/wake 与 per-task robust 状态。

## 子目录但未接入聚合默认构建

- **`ipc-signal`**、**`ipc-shm`**、**`ipc-event`** 等子 crate 在仓库中存在各自 **`Cargo.toml`** 与占位实现，未列入根 **`default`** 依赖图。

## 明确未覆盖

- pipe 的最小用户态 fd/syscall 接线已在 `wateros-syscall` 内完成；仍不承诺完整 Linux pipe ABI，也尚未覆盖 fork/dup 继承语义。
- futex：**`FUTEX_REQUEUE`**、共享（非 private）futex 键空间、信号打断 **`EINTR`** 尚未实现。
- signal、shm、event 等完整 IPC 语义仍未落地。
- **`ipc-api`** 从占位演进为稳定契约并实现非 dummy **`impl-*`**（futex 已具备 **`impl-task`**）。

## 维护要求

将子 crate 纳入 workspace、根依赖或 **`os/src`** 调用时，同步更新本文件、**`docs/architecture/snapshot.md`** 与 **`docs/roadmap/todolist.md`**。
