# wateros-base 新增 impl 指南

## 新增 impl 的基本步骤

该组件以基础类型为主，新增内容时优先扩展稳定基础能力，而不是引入带平台假设的实现。重点检查 `wateros-base/src/lib.rs` 与 `base-config`。

## 通用检查清单

- 新 impl 目录是否加入 workspace members
- impl crate 是否依赖正确的 `api-v0`
- 组件根 `Cargo.toml` 是否新增 feature
- 聚合 `src/lib.rs` 是否新增 `cfg(feature = ...)` 导出
- 相关导出文档是否已同步更新
