# 编写第 1 章：项目概述

## 任务目标

撰写或修订 `chapters/chap01.tex`，让读者快速理解 WaterOS 是什么、设计目标是什么、当前做到哪一步。

## 输出文件

- `docs/technical_document/wateros-latex/chapters/chap01.tex`

## 执行前必读

- `docs/technical_document/prompts/README.md`（通用 LaTeX 约定）
- `docs/prompts/architecture.md`
- `docs/exports/snapshot/current.md`
- `docs/exports/features/` 下各一级组件快照（浏览标题与状态即可）

## 章节结构（必须保留）

```latex
\chapter{项目概述}
  % 章首 2～4 段：项目定位、双架构、组件化一句话
\section{设计目标}
  % itemize 列表，5 条左右，可量化
\section{当前实现摘要}
  % longtable：子系统 | 当前实现状态
```

## 各节写作要点

### 章首段落

- 说明：`no_std` Rust 内核、RISC-V64 + LoongArch64、QEMU bring-up 已打通的主路径（启动 → trap → MM → 调度 → 用户 ELF）
- 强调：平台差异收束在 platform/arch/driver/mm impl；上层 task/VFS/syscall/IPC 复用
- 提及：组件化（`api-v0` / `impl-*` / 聚合 `lib.rs`）

### §1.1 设计目标

每条目标应**可验证**，避免空泛。建议覆盖：

- 可启动、可调度、可运行用户 ELF
- Linux generic 64-bit syscall / ABI 对齐范围
- 双 QEMU 主线：块设备、ext4 根卷、VFS fd
- 可扩展组件边界（平台与上层分离）
- 文档与脚本可复现（feature-tree、exports、Makefile）

### §1.2 当前实现摘要

- 用一张 `longtable`，行至少包含：启动与平台、内存管理、任务与调度、驱动、FS 与 VFS、系统调用、IPC 与信号、验证与复现
- 每格 2～4 句，写**现状**而非规划；骨架/占位须写明（如「占位 impl」「子集」）
- 与 `docs/exports/features/` 和 `docs/exports/snapshot/current.md` 交叉核对

## 事实来源（优先顺序）

1. `os/feature-tree.txt`、`os/Cargo.toml` [features]
2. `docs/exports/snapshot/current.md`
3. `docs/exports/features/*.md`
4. `docs/roadmap/todolist.md`（仅用于标注缺口）

## 禁止

- 不写第 2 章才展开的 feature 传递细节、lib.rs 聚合代码
- 不写第 3 章才展开的 impl 算法与长代码摘录
- 不编造未在源码或 exports 中出现的 syscall/驱动能力

## 完成检查

- [ ] 文件顶部 `%` 说明注释仍在（或已用 `annotate-tex-files.py` 刷新）
- [ ] `\chapter` / `\section` 标题与上一致
- [ ] 表格有 `\caption` 与 `\label{tab:current-summary}`（或等价唯一 label）
- [ ] 术语与第 2、3 章一致（组件名、feature 名与 crate 名统一）
