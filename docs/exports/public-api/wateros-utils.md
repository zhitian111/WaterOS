# wateros-utils 公共 API 快照

## 用途

记录 **`wateros-utils`** 当前对根 crate **`wateros`** 暴露的 **真实符号**。本 crate **无** `[features]`、**无** api/impl 拆分，体量保持为早期占位。

## 事实来源

- [`os/components/wateros-utils/Cargo.toml`](../../os/components/wateros-utils/Cargo.toml)
- [`os/components/wateros-utils/src/lib.rs`](../../os/components/wateros-utils/src/lib.rs)

## 聚合层导出

| 项 | 说明 |
|----|------|
| **`add(left, right) -> u64`** | 根级 **`pub fn`**；源码中标注为模板占位、仅供 crate 内单测，**不代表**最终内核公共 API 承诺。 |

## 缺口说明

- 后续若引入真实工具函数或数据结构，应同步收敛命名、补充 **`//!` 模块语义**，并更新本快照。

## 维护要求

根 **`lib.rs`** 出现新的 **`pub`** 项或 crate 职责变化时，更新本文件。
