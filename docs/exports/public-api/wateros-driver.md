# wateros-driver 公共 API 快照

## 当前定位

聚合层当前导出 `api`、`block`、`character`、`network` 模块，并提供 `init_when_boot`、`init_after_boot` 和 `test` 等入口。默认实现为 `impl-qemu-riscv64-opensbi`。

## 事实来源

- 组件根 `Cargo.toml`
- 组件聚合 `src/lib.rs`
- 对应 `api-v0` 与 `impl-*` 目录

## 维护要求

当聚合层导出项、默认 feature 或组件边界发生变化时，应同步更新本文件。
