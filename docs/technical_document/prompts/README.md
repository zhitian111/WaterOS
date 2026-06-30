# WaterOS 技术文档 — Agent 编写提示词索引

本目录为 WaterOS **LaTeX 技术文档**各章的 Agent 任务说明。分发任务时直接 `@` 对应章节的 `.md` 文件即可。

LaTeX 工程目录：`docs/technical_document/wateros-latex/`

## 文档工程路径

| 项 | 路径 |
|----|------|
| 编译入口 | `wateros-latex/main.tex` |
| 封面 | `wateros-latex/frontmatter/cover-page.tex` |
| 章节正文 | `wateros-latex/chapters/chap01.tex` … `chap05.tex` |
| 第 2 章（当前编入 PDF） | `wateros-latex/chapters/chap02/written-architecture.tex` |
| 第 3 章（当前编入 PDF） | `wateros-latex/chapters/chap03/written-implementation.tex` |
| 第 3 章（模块化目标态） | `wateros-latex/chapters/chap03/components/**` |
| 刷新文件头 `%` 说明 | `wateros-latex/scripts/annotate-tex-files.py` |
| 编译 | `wateros-latex/scripts/build.bash`（产物在 `wateros-latex/build/main.pdf`） |

## 章节任务文件

| 章 | 任务文件 | 主要输出 |
|----|----------|----------|
| 1 项目概述 | [chap01.md](chap01.md) | `wateros-latex/chapters/chap01.tex` |
| 2 总体架构设计 | [chap02.md](chap02.md) | `wateros-latex/chapters/chap02/written-architecture.tex` |
| 3 模块实现 | [chap03.md](chap03.md) | `written-implementation.tex` 或 `components/` |
| 3 组件并行（子任务） | [chap03-modular.md](chap03-modular.md) | `wateros-latex/chapters/chap03/components/wateros-*/**` |
| 4 测试与复现 | [chap04.md](chap04.md) | `wateros-latex/chapters/chap04.tex` |
| 5 总结与后续 | [chap05.md](chap05.md) | `wateros-latex/chapters/chap05.tex` |

## 所有章节通用要求

执行任意章节任务前，至少阅读：

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/documentation.md`
- `docs/prompts/architecture.md`

LaTeX 写作约定（与 `test.tex` / `setup/doc-macros.tex` 一致）：

- 内核/路径/符号：`\code{...}`；系统调用：`\syscall{...}`；项目名：`\wateros{}`
- 代码块（minted + Pygments）：
  - Rust：`\begin{rustcode}...\end{rustcode}`
  - Shell/Makefile：`\begin{bashcode}...`
  - Cargo.toml：`\begin{tomlcode}...`
  - 目录树等纯文本：`\begin{textcode}...`
- 表格用 `longtable` + `booktabs`（`\toprule` 等）
- **保留**文件顶部 `%` 写作说明注释；大改后运行 `annotate-tex-files.py` 仅更新说明、不覆盖正文
- 不写毕设/论文套话；语气为技术方案说明
- **正文文风**：除代码块外，不用括号作插入说明，改用完整句子直述；不用箭头、日式引号等非正式符号，可用“至”“经”“对应”等文字表述因果或顺序
- 事实以源码与 `docs/exports/` 为准；与 `docs/technical_document/test.tex` 冲突时以**当前仓库源码**为准并更新 LaTeX

母本文稿：`docs/technical_document/test.tex`（可用 `wateros-latex/scripts/extract-chapters-from-test.py` 重新拆章，拆后须再核对与源码一致性）。
