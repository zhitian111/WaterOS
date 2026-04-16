# 文档体系续写 Prompt

当需要继续扩展、调整或重组本套文档体系时，应使用本文件作为续写上下文。

## 续写原则

- 先尊重现有目录结构和命名，不轻易推翻已有分类。
- 优先做增量修改，避免无必要的大规模搬迁。
- 新需求应先判断属于 `prompts`、`tasks`、`exports`、`guides`、`roadmap` 还是 `architecture`。
- 若新增文档会影响 Agent 的默认行为，应同步更新 `prompts/README.md` 和 `prompts/general.md`。

## 续写时必须检查的文件

- `docs/README.md`
- `docs/prompts/README.md`
- `docs/tasks/README.md`
- `docs/guides/workflow.md`
- `docs/roadmap/todolist.md`
- `docs/architecture/snapshot.md`

## 续写输出要求

- 说明新增或修改的分类。
- 说明为什么该文档应放在该位置。
- 若新增导出结果目录，说明其来源任务和维护方式。
- 若修改规范，说明会影响哪些后续任务。
