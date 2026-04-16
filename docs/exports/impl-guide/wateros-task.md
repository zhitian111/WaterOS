# wateros-task 新增 impl 指南

## 新增 impl 的基本步骤

新增 impl 时要同时关注 `task` 与 `task-scheduler` 两层结构，明确是推进任务对象本身还是推进调度策略实现。

## 通用检查清单

- 新 impl 目录是否加入 workspace members
- impl crate 是否依赖正确的 `api-v0`
- 组件根 `Cargo.toml` 是否新增 feature
- 聚合 `src/lib.rs` 是否新增 `cfg(feature = ...)` 导出
- 相关导出文档是否已同步更新
