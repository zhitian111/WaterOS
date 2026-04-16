# wateros-runtime 公共 API 快照

## 当前定位

聚合层当前导出 `panic`、`console`、`logging`、`heap_allocator` 四类运行时能力。控制台组件支持通过 firmware OpenSBI 路径接入具体实现。

## 事实来源

- 组件根 `Cargo.toml`
- 组件聚合 `src/lib.rs`
- 对应 `api-v0` 与 `impl-*` 目录

## 维护要求

当聚合层导出项、默认 feature 或组件边界发生变化时，应同步更新本文件。
