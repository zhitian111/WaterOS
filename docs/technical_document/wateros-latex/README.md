# WaterOS LaTeX 技术文档

内核技术方案文档。版式、封面与正文初稿来自 `docs/technical_document/test.tex`。

**Agent 分章编写**：见 [../prompts/README.md](../prompts/README.md)，按章 `@` 对应任务文件即可。

## 编译

```bash
cd docs/technical_document/wateros-latex
./scripts/build.bash          # 生成 build/main.pdf
./scripts/build.bash watch    # 保存时自动重编
./scripts/build.bash clean    # 清理 build/ 与遗留中间文件
```

或手动（产物同样在 `build/`）：

```bash
latexmk -xelatex -shell-escape -outdir=build -interaction=nonstopmode main.tex
```

若提示找不到 `xelatex.fmt`：`fmtutil-user --byfmt xelatex`  
Arch 分包说明见 [docs/INSTALL-arch.md](docs/INSTALL-arch.md)。

## 目录结构

```text
wateros-latex/
├── main.tex                    # 入口（封面 + 目录 + 五章）
├── build/                      # 编译产物（PDF、log、minted 缓存等，git 忽略内容）
├── .latexmkrc                  # 默认 -outdir=build
├── frontmatter/cover-page.tex  # 封面
├── setup/
│   ├── package.tex             # 宏包（自原 Thesis 模板）
│   ├── format.tex              # 版式
│   └── doc-macros.tex          # 字体、代码高亮、\wateros 等宏
├── figures/cover.jpg
├── chapters/
│   ├── chap01.tex … chap05.tex           # 从 test.tex 拆出的正文
│   ├── chap02/written-architecture.tex
│   ├── chap03/written-implementation.tex
│   └── chap03/components/                # 按模块树拆分的扩展骨架（待迁移）
└── scripts/
    ├── extract-chapters-from-test.py     # 从 test.tex 重新抽取章节
    ├── gen-component-skeleton.bash
    └── annotate-tex-files.py
```

## 正文与模块化

当前 `main.tex` 使用 `test.tex` 中已写好的五章内容。`chapters/chap03/components/` 下是按 `os/components/` 镜像的模块化骨架，后续将 `written-implementation.tex` 中的内容逐步迁入各组件 `.tex`。

从 `test.tex` 更新章节正文：

```bash
./scripts/extract-chapters-from-test.py
```

## LaTeX 写作说明（`%` 注释）

每个 `.tex` 文件顶部有 **LaTeX 注释**（以 `%` 开头），说明该文件应写什么；编译时会被忽略，不影响 PDF。

刷新全部文件的说明（**不删正文**，只替换文件开头注释块）：

```bash
./scripts/annotate-tex-files.py
```

## 事实来源

- `docs/technical_document/test.tex`（当前正文母本）
- 当前仓库源码、`os/feature-tree.txt`
