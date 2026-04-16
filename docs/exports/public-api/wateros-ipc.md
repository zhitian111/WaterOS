# wateros-ipc 公共 API 快照

## 当前定位

组件已拆出 `ipc-pipe`、`ipc-signal`、`ipc-futex`、`ipc-event`、`ipc-shm`、`ipc-waitqueue` 等子模块，但总入口和多个子模块仍存在骨架状态。

## 事实来源

- 组件根 `Cargo.toml`
- 组件聚合 `src/lib.rs`
- 对应 `api-v0` 与 `impl-*` 目录

## 维护要求

当聚合层导出项、默认 feature 或组件边界发生变化时，应同步更新本文件。
