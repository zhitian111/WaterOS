# wateros-mm 新增 impl 指南

## 新增 impl 的基本步骤

新增 impl 时需要更新 `wateros-mm/Cargo.toml`、`mm-impl/impl-*/Cargo.toml`、必要时 `mm-frame-alloctor` 的实现选择，以及聚合 `src/lib.rs` 中的导出别名和测试路径。

## 通用检查清单

- 新 impl 目录是否加入 workspace members
- impl crate 是否依赖正确的 `api-v0`
- 组件根 `Cargo.toml` 是否新增 feature
- 聚合 `src/lib.rs` 是否新增 `cfg(feature = ...)` 导出
- 相关导出文档是否已同步更新
