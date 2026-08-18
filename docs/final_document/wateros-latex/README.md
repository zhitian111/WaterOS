# WaterOS决赛设计文档

## 编译

```bash
cd docs/final_document/wateros-latex
./scripts/build.bash build
```

PDF 输出到 `build/main.pdf`。脚本会启用 XeLaTeX、shell escape 和 minted 所需选项。
若缺少 Pygments，请先执行 `python3 -m pip install --user Pygments`。清理使用：

```bash
./scripts/build.bash clean
```

## 写作原则

- 正文按“问题、设计约束、实现、验证”组织，不按源码目录逐项罗列。
- `data/evidence.tsv` 是结论与证据的审计台账；`candidate-rerun` 状态必须在提交前清零。
- 历史实验与候选版本结果分开表述，不用历史最好值代替最终候选复验。
- 详细 AI 代码清单在候选提交冻结后重新生成，不沿用初赛的行数和占比。
- 性能优化方案和实验数据由单独文档维护，不在本设计文档重复展开。

## 目录

```text
wateros-latex/
├── main.tex
├── setup/document.tex
├── data/evidence.tsv          # 内部审计台账，不进入 PDF
├── frontmatter/cover.tex
├── chapters/
│   ├── 01-evolution.tex       # 项目概述
│   ├── 02-architecture.tex    # 总体架构设计
│   ├── 03-final-design.tex    # 关键模块实现
│   ├── 04-workloads.tex       # 测试与调试工具
│   └── 07-conclusion.tex      # 总结与后续工作
```
