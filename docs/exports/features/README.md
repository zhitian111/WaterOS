# 功能快照目录说明

## 用途

本目录按一级组件拆分 **`docs/exports/features/*.md`**，记录默认 feature 组合下的能力边界、事实来源路径与维护要求，便于与 **`docs/architecture/snapshot.md`** 对照阅读。

## 与源码 Rustdoc 的关系

组件内 **`//!`（crate/模块）** 与 **`///`（对外 `pub` 项）** 承担**语义契约**与分层边界说明，编写要求见 **`docs/guides/documentation.md`** 与 **`docs/prompts/documentation.md`**。

功能快照文件侧重**工程事实**（依赖、feature 链、未覆盖项）；若与某类型或函数的精确契约存在表述差异，**以对应 crate 源码中的 rustdoc 为准**（含非默认 feature、dummy impl 等全部子 crate），并应在评审周期内收敛导出文字。
