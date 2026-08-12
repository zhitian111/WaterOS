# WaterOS 技术文档

[项目首页](../../README.md) · [文档总览](../README.md) · [LaTeX 工程](./wateros-latex/README.md)

内核技术方案 LaTeX 文档与 Agent 编写说明。

| 目录 | 说明 |
|------|------|
| [wateros-latex/](wateros-latex/) | LaTeX 工程（`main.tex`、章节、编译脚本） |
| [prompts/](prompts/) | **各章 Agent 编写提示词**（`chap01.md` … `chap05.md`） |
| [test.tex](test.tex) | 正文母本（拆章前参考） |

## 快速开始

**编译 PDF**

```bash
cd docs/technical_document/wateros-latex
./scripts/build.bash
# PDF: build/main.pdf
```

**让 Agent 写某一章**

在对话中 `@docs/technical_document/prompts/chap03.md`（或对应章节文件），并说明模式 A/B（见 `chap03.md`）。

## 章节与提示词对照

| 章 | 提示词 | LaTeX 输出 |
|----|--------|------------|
| 1 项目概述 | [prompts/chap01.md](prompts/chap01.md) | `wateros-latex/chapters/chap01.tex` |
| 2 总体架构 | [prompts/chap02.md](prompts/chap02.md) | `wateros-latex/chapters/chap02/written-architecture.tex` |
| 3 模块实现 | [prompts/chap03.md](prompts/chap03.md) | `written-implementation.tex` 或 `components/` |
| 3 组件并行 | [prompts/chap03-modular.md](prompts/chap03-modular.md) | `chapters/chap03/components/**` |
| 4 测试复现 | [prompts/chap04.md](prompts/chap04.md) | `wateros-latex/chapters/chap04.tex` |
| 5 总结 | [prompts/chap05.md](prompts/chap05.md) | `wateros-latex/chapters/chap05.tex` |

详细约定见 [prompts/README.md](prompts/README.md)。
