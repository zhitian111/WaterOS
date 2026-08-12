# 可复用任务提示词

[项目首页](../../../README.md) · [文档总览](../../README.md) · [Agent 文档](../README.md)

本目录存放可直接提供给 Agent 的通用工作说明，不绑定某一轮缺陷或性能实验。

- `analyze_kernel_log.md`：分析 QEMU/内核日志。
- `audit_*.md`：系统调用、锁和资源生命周期审计。
- `commenting.md`：代码注释与 rustdoc 整理。
- `export_*.md`：生成架构、feature、API、实现和发布概览。
- `ltp_*.md`：LTP 自动迭代与 fast-exit 分析。
- `run_testsuits_qemu.md`：分阶段 QEMU 测例运行与判读。

当前问题、性能实施和专项回归分别位于 `docs/tasks/known-issues/`、
`docs/tasks/perf/` 和 `docs/tasks/read-family/`。每项任务的已完成记录保留在其自身的
`history/` 或 `reports/` 目录。
