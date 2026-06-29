# 编写第 2 章：总体架构设计

## 任务目标

撰写或修订 **总体架构** 正文：说明 WaterOS 如何按组件/feature/API/聚合层组织，以及双架构下如何保持上层一致。

## 输出文件

- **当前编入 PDF**：`chapters/chap02/written-architecture.tex`
- （备用模块化）`chapters/chap02/design-philosophy.tex` 等 — 仅当 `main.tex` 切换为 `\input` 聚合时再写

## 执行前必读

- `docs/technical_document/prompts/README.md`
- `docs/prompts/architecture.md`
- `docs/exports/architecture/module-relations.md`
- `docs/exports/architecture/components.md`
- `os/feature-tree.txt`
- `os/src/main.rs`（启动顺序）

## 章节结构（必须保留 `\chapter{总体架构设计}` 及以下 section 顺序）

| Section | 内容要点 |
|---------|----------|
| 组件化分层 | 三层表：平台与运行时 / 内核资源 / 用户接口；各层组件列表 |
| 代码结构 | `textcode` 目录树：`os/`、`components/`、`main.rs`、`feature-tree.txt` |
| 组件目录与职责映射 | `longtable`：每个 `wateros-*` 一级组件一行职责 |
| API 与实现解耦 | api-v0、impl-*、聚合 lib.rs 三步；enumerate 说明 |
| 启动主线 | enumerate：从 `kernel_main` 到 `run_first_task` 的顺序 |
| Feature 组合与平台构建 | 摘录或概括 `os/Cargo.toml` 中 `qemu-riscv64-opensbi` / `qemu-loongarch64-virt` |
| QEMU 双架构适配与一致性 | 对比表：启动参数、trap、页表、设备总线、上层一致性 |
| 内核入口代码流程 | 两段 `rustcode`：`kernel_main` RV 与 LA 主线（可与源码同步） |

## 写作原则

- 本章讲 **设计与组织**，不讲具体页表 walk、调度器数据结构等实现细节（留给第 3 章）
- 所有组件依赖关系以 **聚合门面** 为准（`mm::`、`vfs::`、`ipc::`），不画未在 `Cargo.toml` 出现的依赖
- Feature 叙述必须能对上 `os/feature-tree.txt` 中的链；可举一条 RV、一条 LA 完整传递示例
- 代码树与 `os/components/` 实际目录一致；新增/删除组件须同步改表

## 事实来源

- `os/Cargo.toml`、`os/feature-tree.txt`
- 各组件 `src/lib.rs`（仅看 `pub mod` 树与 `active_impl`）
- `docs/exports/public-api/`（对外模块名）
- `docs/guides/filesystem-current.md` 等 guides（仅 FS 相关节引用）

## 禁止

- 不把第 3 章的长 syscall 实现、`trap_handler` 全文塞入本章（启动节可摘录 **精简** 的 `kernel_main`）
- 不描述已删除的组件或旧路径（如仅以历史 test.tex 为准而不核对源码）

## 完成检查

- [ ] 两张以上 `longtable` / `rustcode` 可编译（minted 代码块内 `_` 无需转义）
- [ ] Feature 列表与当前 `os/Cargo.toml` 一致
- [ ] 双架构对比表与第 1 章、第 4 章表述不矛盾
- [ ] 文件顶部 `%` 说明注释保留

## 模块化迁移（可选后续任务）

若将本章拆为 `chap02/*.tex` 多文件：

1. 按上表每节一个 `.tex`
2. `architecture.tex` 仅 `\chapter` + `\input{...}`
3. 修改 `main.tex` 用 `\include{chapters/chap02/architecture}` 替换 `written-architecture`
