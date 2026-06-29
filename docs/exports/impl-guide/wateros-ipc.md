# wateros-ipc — 新增 impl 指南

## 用途

说明在 `wateros-ipc` 组件内新增或替换 API/impl 时需要修改的文件、feature 接线与 trait 契约。事实来源：各子 crate 的 `Cargo.toml` 与 `src/lib.rs`。

## 总体结构

```text
wateros-ipc/
  src/lib.rs              # 聚合重导出
  ipc-api/api-v0/         # 顶层 IPC API（当前占位）
  ipc-impl/impl-dummy/    # 顶层 active_impl 占位
  ipc-waitqueue/          # waitqueue 子系统
  ipc-pipe/               # pipe 子系统
  ipc-futex/              # futex 子系统
  ipc-shm/                # shm（无 impl 分叉，逻辑在单 crate）
  ipc-signal/             # signal（逻辑在聚合 crate，impl-dummy 仅占位）
  ipc-event/              # 事件占位（尚未挂接）
```

## 新增子系统 impl 的通用步骤

1. 在 `*-api/api-v0/` 定义稳定 trait、错误类型与常量。
2. 在 `*-impl/impl-<name>/` 实现 trait；必要时依赖 `ipc-waitqueue`。
3. 在中间聚合 crate（如 `ipc-pipe`）的 `Cargo.toml` 增加 optional 依赖与 feature。
4. 在中间聚合 `src/lib.rs` 用 `#[cfg(feature = "impl-<name>")] pub use impl_<name> as active_impl`。
5. 在 `wateros-ipc/Cargo.toml` 增加 optional 依赖、feature 传递，并在 `src/lib.rs` 条件 `pub mod`。
6. 在根 `os/Cargo.toml` 的 `impl-riscv64` / `impl-loongarch64` 中按需启用 feature。
7. 补充 `ipc::<subsystem>::test()` 自检并更新 `docs/exports/features/wateros-ipc.md`。

## waitqueue（`impl-task`）

| 项 | 路径 |
|----|------|
| API trait | `ipc-waitqueue/waitqueue-api/api-v0` → `IpcWaitQueueOps` |
| 实现 | `ipc-waitqueue/waitqueue-impl/impl-task` → `WaitQueue` |
| Feature | `ipc-waitqueue/impl-task`（默认开启） |

**替换 impl 时：**

- 新 impl 必须实现 `IpcWaitQueueOps` 或提供与 `WaitQueue` 相同 inherent API（futex/pipe 直接调用 inherent 方法）。
- 保持 `WaitQueueId` / `TaskWaitResult` 与 `wateros-task` 语义一致。

## pipe（`impl-ringbuf`）

| 项 | 路径 |
|----|------|
| Traits | `PipeEndpointOps`、`KernelPipe`（`pipe-api/api-v0`） |
| 实现 | `pipe-impl/impl-ringbuf` → `Pipe`、`PipeEndpoint` |
| Feature | `ipc-pipe/impl-ringbuf`；聚合 `wateros-ipc/pipe` |

**新 pipe impl 必须实现：**

- `KernelPipe::with_capacity`、`try_read`/`read`、`try_write`/`write`、`close_read`/`close_write`
- `PipeEndpointOps::pair`、`read`/`write`/`close`、`poll_revents`、`poll_wait_for_ticks`
- 与 `waitqueue::WaitQueue` 集成的阻塞语义

## futex（`impl-task` / `impl-dummy`）

| 项 | 路径 |
|----|------|
| Trait | `KernelFutexOps`（`futex-api/api-v0`） |
| 生产 impl | `futex-impl/impl-task` → `FutexHub` |
| 占位 impl | `futex-impl/impl-dummy` → 返回 `Nosys` |
| Feature | `ipc-futex/impl-task`（默认）；`wateros-ipc/futex` → `impl-task` |

**`impl-task` 关键点：**

- `FutexHub::global()` 单例 + `BTreeMap<FutexKey, WaitQueue>`
- `wait_while` 为 inherent 方法（trait 不含闭包等待）
- robust：`set_robust_list` 校验 `ROBUST_LIST_HEAD_SIZE == 24`

**切换 dummy：**

- `ipc-futex` 启用 `impl-dummy` 且关闭 `impl-task`
- 仅用于无调度依赖的链接测试

## shm / signal

- **shm**：无独立 impl crate；扩展时直接改 `ipc-shm/src/lib.rs`，保持 `registry()` 入口稳定。
- **signal**：主逻辑在 `ipc-signal/src/lib.rs`；`signal-impl/impl-dummy` 仅为占位，未作为 `active_impl` 导出。新增 impl 时应将 `SignalRegistry`  trait 化或拆分到 `signal-impl/impl-*`。

## 顶层 `active_impl`（`impl-dummy`）

- 路径：`ipc-impl/impl-dummy`
- Feature：`wateros-ipc/impl-dummy`（默认）
- 当前仅 `add()` 占位；替换真实顶层 impl 时保持 `ipc::active_impl` 路径稳定。

## 检查清单

- [ ] `api-v0` trait/类型有 `///` 中文契约说明
- [ ] 薄包装/转发函数加 `#[inline]`
- [ ] 聚合 `src/lib.rs` 模块树与 `Cargo.toml` feature 一致
- [ ] `impl-riscv64`/`impl-loongarch64` feature 传递已更新
- [ ] `self_tests` 或子 crate `test()` 可编译
- [ ] 更新 `features` / `public-api` 导出文档

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版 impl 指南 |
