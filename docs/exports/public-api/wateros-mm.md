# wateros-mm 公共 API 快照

## 当前定位

聚合层当前导出 `api`，并根据 feature 在 `impl-sv39` 与 `impl-dummy` 间选择 `mm_impl`。同时通过 `frame_alloctor` 组织帧分配能力和自检。

## 事实来源

- 组件根 `Cargo.toml`
- 组件聚合 `src/lib.rs`
- 对应 `api-v0` 与 `impl-*` 目录

## 维护要求

当聚合层导出项、默认 feature 或组件边界发生变化时，应同步更新本文件。
