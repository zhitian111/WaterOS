# wateros-ipc 新增 impl 指南

## 新增 impl 的基本步骤

新增 impl 时优先针对子组件推进，例如 pipe 或 signal，再回到总 IPC 聚合层补齐 feature 和导出。需要同步检查 `wateros-ipc/Cargo.toml` 与各子组件 `Cargo.toml`。当前 `ipc-pipe` 作为 path dependency 接入 `wateros-ipc`，不再保留独立 nested workspace root，避免 Cargo 嵌套 workspace 冲突。

## 通用检查清单

- 新 impl 目录是否需要加入 workspace members；若子组件自身曾是 workspace root，应先避免嵌套 workspace
- impl crate 是否依赖正确的 `api-v0`
- 组件根 `Cargo.toml` 是否新增 feature
- 聚合 `src/lib.rs` 是否新增 `cfg(feature = ...)` 导出
- 相关导出文档是否已同步更新
