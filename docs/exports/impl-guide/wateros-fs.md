# wateros-fs 新增 impl 指南

## 新增 impl 的基本步骤

新增 impl 时需要同时更新 `wateros-fs/Cargo.toml`、`fs-impl/impl-*/Cargo.toml` 和聚合导出层。重点是明确与 `vfs` 的边界，以及 impl 如何挂接到默认 feature。

## 通用检查清单

- 新 impl 目录是否加入 workspace members
- impl crate 是否依赖正确的 `api-v0`
- 组件根 `Cargo.toml` 是否新增 feature
- 聚合 `src/lib.rs` 是否新增 `cfg(feature = ...)` 导出
- 相关导出文档是否已同步更新
