# wateros-abi 公共 API 快照

## 当前定位

当前主要承载 ABI、系统调用约定和用户内核接口语义。根组件默认仍偏骨架，但已经具备 `api-v0`、`impl-dummy` 和 `impl-linux-riscv64` 的组织结构。

## 事实来源

- 组件根 `Cargo.toml`
- 组件聚合 `src/lib.rs`
- 对应 `api-v0` 与 `impl-*` 目录

## 维护要求

当聚合层导出项、默认 feature 或组件边界发生变化时，应同步更新本文件。
