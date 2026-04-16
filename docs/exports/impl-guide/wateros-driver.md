# wateros-driver 新增 impl 指南

## 新增 impl 的基本步骤

新增 impl 时至少要更新 `wateros-driver/Cargo.toml`、对应 impl crate 的 `Cargo.toml`、聚合 `src/lib.rs` 以及需要接入的子系统组件。若是平台驱动实现，需同步检查平台和 DTB 初始化链。

## 通用检查清单

- 新 impl 目录是否加入 workspace members
- impl crate 是否依赖正确的 `api-v0`
- 组件根 `Cargo.toml` 是否新增 feature
- 聚合 `src/lib.rs` 是否新增 `cfg(feature = ...)` 导出
- 相关导出文档是否已同步更新
