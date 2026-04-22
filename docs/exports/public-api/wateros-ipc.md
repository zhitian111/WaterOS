# wateros-ipc 公共 API 快照

## 当前定位

组件已拆出 `ipc-pipe`、`ipc-signal`、`ipc-futex`、`ipc-event`、`ipc-shm`、`ipc-waitqueue` 等子模块。当前总入口仍偏早期，但 `ipc-waitqueue` 已开始作为 `wateros-task::WaitQueue` 的 IPC 语义包装层落地，并可继续暴露底层 `TaskWaitHandle`，为后续 event/pipe/futex 复用统一等待对象提供入口。

## 事实来源

- 组件根 `Cargo.toml`
- 组件聚合 `src/lib.rs`
- 对应 `api-v0` 与 `impl-*` 目录

## 维护要求

当聚合层导出项、默认 feature 或组件边界发生变化时，应同步更新本文件。
