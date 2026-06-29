# 编写第 3 章：模块实现（按组件树并行）

## 任务目标

按 `os/components/` 镜像目录，为 **单个一级组件**（或其子树）撰写 LaTeX 正文，填入 `chapters/chap03/components/` 下对应 `.tex` 文件。

适用于多 Agent **并行**：每个任务只负责一个 `wateros-<name>` 或一个子模块子树。

## 输出范围

```
chapters/chap03/components/wateros-<component>/
├── <short>.tex              % 一级组件 § 综述 + \input 子模块
├── <submodule>/
│   ├── <short>.tex          % \subsection 综述 + \input api/impl
│   ├── ...-api/api/api-v0.tex
│   └── ...-impl/impl/<variant>.tex
```

命名约定见 `docs/technical_document/wateros-latex/README.md`（不用 `index.tex`）。

## 执行前必读

- `docs/technical_document/prompts/README.md`
- `docs/technical_document/prompts/chap03.md`（整章脉络）
- `docs/prompts/architecture.md`
- 目标组件的 `docs/exports/features/wateros-<name>.md`
- 目标组件的 `docs/exports/public-api/wateros-<name>.md`
- 目标路径下 `.tex` 文件顶部的 `%` 写作说明（`annotate-tex-files.py` 生成）

## 撰写顺序（单组件内）

1. **叶子**：`api/api-v0.tex`（只写契约）、`impl/<variant>.tex`（只写实现）
2. **子模块聚合**：`<submodule>.tex` 短综述 + `\input` 叶子
3. **一级组件**：`<short>.tex` 的 `\section{wateros-...}` + `\input` 子模块

## 各层写什么

| 层级 | 写什么 | 不写什么 |
|------|--------|----------|
| api-v0 叶子 | trait、类型、错误、语义契约、上下文限制 | impl 内部结构、汇编 |
| impl 叶子 | 数据结构、算法、feature 链、与 api 的对应 | 重复 api trait 全文 |
| 子模块聚合 | 职责、边界、lib.rs 如何 re-export | 跨组件长篇背景 |
| 一级组件 § | 在内核中的位置、根 feature 如何选用、依赖谁 | 其他组件实现细节 |

## 任务拆分示例

分发时在任务描述中**写死路径**，例如：

- 「只写 `wateros-ipc/ipc-waitqueue/` 子树」
- 「只写 `wateros-platform/platform-arch/arch-impl/impl/loongarch64.tex`」
- 「写满整个 `wateros-mm` 一级组件」

## 事实来源

- `os/components/wateros-<name>/**` 全部 `Cargo.toml` 与 `src/`
- `os/feature-tree.txt` 中该组件相关段
- `docs/exports/architecture/module-relations.md` 对应表

## LaTeX 约定

- 一级组件根文件使用 `\section{wateros-xxx}`（在第 3 章 `\chapter` 之下）
- 子模块用 `\subsection{...}`；更深层可用 `\subsubsection`（ sparingly）
- 保持已有 `\input{chapters/chap03/components/...}` 路径不变
- 大段从 `written-implementation.tex` 迁移时，改成组件视角后删除整章内重复段落

## 禁止

- 不修改其他组件目录下的 `.tex`（除非任务明确要求）
- 不把多个一级组件写进同一个任务（避免冲突）
- 不运行 `annotate-tex-files.py` 覆盖其他 Agent 正在编辑的文件（仅改自己负责路径）

## 完成检查

- [ ] 负责路径下所有叶子均有实质正文（非仅 `% TODO`）
- [ ] 聚合 `.tex` 的 `\input` 链完整、无断链
- [ ] 与 `docs/exports/` 一致；新能力须核对源码而非旧 test.tex
- [ ] 组件写完后，在 `implementation.tex` 中该 `\input` 已存在（默认已有）

## 整章切换

当所有一级组件 `components/` 写满后：

1. 确认 `implementation.tex` 聚合完整
2. `main.tex` 将 `written-implementation` 换为 `implementation`
3. 全文编译；删或归档 `written-implementation.tex` 中已迁移的重复内容
