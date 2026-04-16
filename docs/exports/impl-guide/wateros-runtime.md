# wateros-runtime 新增 impl 指南

## 新增 impl 的基本步骤

新增 impl 时需要按子组件推进，例如 console、logging 或 heap allocator。检查点包括 `wateros-runtime/Cargo.toml`、对应子组件 `Cargo.toml` 和聚合 `src/lib.rs`。

## 通用检查清单

- 新 impl 目录是否加入 workspace members
- impl crate 是否依赖正确的 `api-v0`
- 组件根 `Cargo.toml` 是否新增 feature
- 聚合 `src/lib.rs` 是否新增 `cfg(feature = ...)` 导出
- 相关导出文档是否已同步更新
