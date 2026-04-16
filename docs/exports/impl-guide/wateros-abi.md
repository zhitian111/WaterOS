# wateros-abi 新增 impl 指南

## 新增 impl 的基本步骤

新增 impl 时，需要检查 `os/components/wateros-abi/Cargo.toml`、`abi-impl/impl-*/Cargo.toml` 和聚合 `src/lib.rs`。实现重点通常是系统调用编号、错误码、用户态约定和与 Linux RISC-V 的对齐。

## 通用检查清单

- 新 impl 目录是否加入 workspace members
- impl crate 是否依赖正确的 `api-v0`
- 组件根 `Cargo.toml` 是否新增 feature
- 聚合 `src/lib.rs` 是否新增 `cfg(feature = ...)` 导出
- 相关导出文档是否已同步更新
