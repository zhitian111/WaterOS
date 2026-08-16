# 当前任务与历史记录

[项目首页](../../README.md) · [文档总览](../README.md) · [标准流程](../workflows/README.md)

`docs/tasks/` 只保留与当前代码状态直接相关的执行入口：

- `known-issues/`：尚未闭环的正确性、兼容性和交付问题。
- `perf/`：仍待执行的性能 wave 任务。
- `read-family/`：尚未完成的 read 调用族集成回归。
- `real-hardware-port/`：真实板卡移植的当前结论与真机闭环报告。
- 每个任务目录中的 `history/`：已完成任务的验证记录、实验与交接记录。
- 每个任务目录中的 `reports/`：面向任务交付的汇总报告；跨任务报告在
  `cross-task-reports/reports/`。

可复用的 Agent 审计、导出、日志分析、LTP 和 QEMU 测试提示词位于
[`docs/agents/tasks/`](../agents/tasks/README.md)。
