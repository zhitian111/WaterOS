# wateros-platform 新增 impl 指南

## 新增 impl 的基本步骤

新增 impl 时必须同时检查 `wateros-platform/Cargo.toml`、`platform-impl`、`platform-arch`、`platform-firmware` 的 feature 传递链。实现中通常需要定义 BootArgs、时间能力和固件调用桥接。

## 通用检查清单

- 新 impl 目录是否加入 workspace members
- impl crate 是否依赖正确的 `api-v0`
- 组件根 `Cargo.toml` 是否新增 feature
- 聚合 `src/lib.rs` 是否新增 `cfg(feature = ...)` 导出
- 相关导出文档是否已同步更新
