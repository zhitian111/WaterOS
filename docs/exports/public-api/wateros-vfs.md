# wateros-vfs 公共 API 快照

## 当前定位

当前已具备 `vfs-api` 与 `vfs-impl/impl-dummy` 结构，但聚合层仍偏早期模板。后续需要承接 fs 与 fd 语义。

## 事实来源

- 组件根 `Cargo.toml`
- 组件聚合 `src/lib.rs`
- 对应 `api-v0` 与 `impl-*` 目录

## 维护要求

当聚合层导出项、默认 feature 或组件边界发生变化时，应同步更新本文件。
