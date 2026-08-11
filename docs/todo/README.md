# 当前待办与分析资料

`docs/todo/` 只保留仍用于判断下一步工作的性能分析，以及由原作者维护的个人待办；可直接执行的
任务位于 [`../tasks/README.md`](../tasks/README.md)，可复用的 Agent 提示词位于
[`../agents/README.md`](../agents/README.md)。

## 当前入口

- [`perf-baseline-gap-report.md`](./perf-baseline-gap-report.md)、
  [`perf-risk-assessment.md`](./perf-risk-assessment.md)：性能决策的基线和风险口径。
- `perf-{hotpath,memory,fs-vfs,ipc-sync,lock-resource}.md` 与
  `perf-fork-exit-degradation.md`：按子系统保存的性能分析与候选项。
- [`kasss's_todo_list/`](./kasss's_todo_list/)：队友个人资料，原作者后续自行维护。

## 归档规则

- 已完成任务的定义文件从本目录删除；验证结果、实验、简报和交接记录归入对应任务的
  `docs/tasks/**/history/` 或 `reports/`。
- 跨任务的阶段简报归入
  [`../tasks/cross-task-reports/reports/`](../tasks/cross-task-reports/reports/)。
- 任务状态以 `docs/tasks/known-issues/`、`perf/` 和 `read-family/` 为准；本目录的
  分析不得重新创建平行任务清单。
