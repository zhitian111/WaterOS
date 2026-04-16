# wateros-fs 公共 API 快照

## 当前定位

当前组件已有 `api-v0`、`impl-dummy` 和 `impl-devfs` 结构，但聚合层仍处于模板或早期阶段，公共 API 需要进一步收敛。

## 事实来源

- 组件根 `Cargo.toml`
- 组件聚合 `src/lib.rs`
- 对应 `api-v0` 与 `impl-*` 目录

## 维护要求

当聚合层导出项、默认 feature 或组件边界发生变化时，应同步更新本文件。
