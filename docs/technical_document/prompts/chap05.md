# 编写第 5 章：总结与后续工作

## 任务目标

总结项目阶段成果与特色，列出后续工作，并填写来源说明与 AI 使用说明（后者可为占位）。

## 输出文件

- `docs/technical_document/wateros-latex/chapters/chap05.tex`

## 执行前必读

- `docs/technical_document/prompts/README.md`
- `docs/roadmap/todolist.md`
- `docs/exports/snapshot/current.md`
- `docs/exports/ai-usage-inventory.tsv`（AI 使用说明节）
- 第 1～4 章已写内容（避免总结与正文矛盾）

## 章节结构

```latex
\chapter{总结与后续工作}
\section{工作总结与项目特色}
\section{后续完善方向}
\section{非本队来源说明}
\section{AI 工具使用说明}
```

## 各节写作要点

### 工作总结与项目特色

- 用 `itemize`，每条 `\wosstrong{标题。}` + 1～2 句（与 test.tex 风格一致）
- 特色建议：组件化组织、双架构边界、用户态路径连续、诊断入口完整
- 与第 1 章摘要、第 3 章实现**呼应**，不引入新未证事实

### 后续完善方向

- 用 `enumerate`，6 条左右，与 `docs/roadmap/todolist.md` 对齐
- 每条可执行、可排期（双架构同构验证、信号/线程、ext4 写路径、资源回收、网络、工具链）

### 非本队来源说明

- 列出第三方 crate（ext4、smoltcp、virtio 等）与 `vendor/` 边界
- 说明 WaterOS 自研部分：平台适配、VFS 封装、syscall、feature 组织等
- 若比赛/课程有披露要求，按实际填写；信息不足时保留简短占位并标注待补

### AI 工具使用说明

- **默认占位**：写明「本节后续填写」即可，除非任务明确要求展开
- 若展开：引用 `docs/prompts/README.md`、`docs/tasks/` 任务索引；说明 Agent 修改内核时的同步清单（feature-tree、exports、本 LaTeX 文档）
- 与 `docs/exports/ai-usage-inventory.tsv` 保持一致

## 事实来源

- `docs/roadmap/todolist.md`
- `os/Cargo.toml` dependencies / vendor
- `docs/exports/ai-usage-inventory.tsv`
- 前文四章

## 禁止

- 不把 roadmap 全文粘贴进 LaTeX
- 不虚构第三方许可证或未使用的 AI 工具

## 完成检查

- [ ] 总结条目不超出第 1～4 章已描述能力范围
- [ ] 后续工作与 todolist 无明显冲突
- [ ] AI 节：占位或已与 inventory 同步
- [ ] 文件顶部 `%` 说明注释保留
