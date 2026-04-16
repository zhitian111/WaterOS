# wateros-platform 公共 API 快照

## 当前定位

聚合层当前导出 `boot`、`arch`、`time`、`timer`、`reset`、`console`、`interrupt` 等能力，并通过 feature 绑定 `impl-qemu-riscv64-opensbi`。这是最典型的 API/impl/聚合范式实现。

## 事实来源

- 组件根 `Cargo.toml`
- 组件聚合 `src/lib.rs`
- 对应 `api-v0` 与 `impl-*` 目录

## 维护要求

当聚合层导出项、默认 feature 或组件边界发生变化时，应同步更新本文件。
