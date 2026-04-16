# wateros-utils 新增 impl 指南

## 新增 impl 的基本步骤

新增内容时优先放置与多个组件共享、但不适合落入具体业务组件的通用能力。

## 通用检查清单

- 新 impl 目录是否加入 workspace members
- impl crate 是否依赖正确的 `api-v0`
- 组件根 `Cargo.toml` 是否新增 feature
- 聚合 `src/lib.rs` 是否新增 `cfg(feature = ...)` 导出
- 相关导出文档是否已同步更新
