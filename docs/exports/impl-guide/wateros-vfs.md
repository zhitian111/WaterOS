# wateros-vfs 新增 impl 指南

## 新增 impl 的基本步骤

新增 impl 时需要明确和 `wateros-fs` 的职责边界，检查 `wateros-vfs/Cargo.toml`、`vfs-api/api-v0`、`vfs-impl/impl-*` 以及聚合层。

## 页缓存 impl（`vfs-impl/impl-page-cache`）

- 依赖 `wateros-base-config::fs` 常量；通过 `impl-fs-bridge` 的 `bridge-fs-api` feature 拉入。
- 暴露 `GlobalFilePageCache`、`global_cache(mount_gen)`、`PageCacheIo` trait。
- 与 `driver-block` 的 LBA 缓存分层，勿合并实现。

## 通用检查清单

- 新 impl 目录是否加入 workspace members
- impl crate 是否依赖正确的 `api-v0`
- 组件根 `Cargo.toml` 是否新增 feature
- 聚合 `src/lib.rs` 是否新增 `cfg(feature = ...)` 导出
- 相关导出文档是否已同步更新
