# 可复用任务提示词

[项目首页](../../../README.md) · [文档总览](../../README.md) · [Agent 文档](../README.md)

本目录存放可直接提供给 Agent 的通用工作说明，不绑定某一轮缺陷或性能实验。

- `analyze_kernel_log.md`：分析 QEMU/内核日志。
- `audit_*.md`：系统调用、锁和资源生命周期审计。
- `commenting.md`：代码注释与 rustdoc 整理。
- `export_*.md`：生成架构、feature、API、实现和发布概览。
- `export_to_gitlab.md`：将指定源提交的 `docs/` 与 `os/` 安全导出、离线验证并推送到 GitLab。
- `ltp_*.md`：LTP 自动迭代与 fast-exit 分析。
- `run_testsuits_qemu.md`：分阶段 QEMU 测例运行与判读。
- `CODEX_HANDOFF_EXPORT_PROMPT.md`：从当前对话导出可验证的任务交接。
- `CODEX_HANDOFF_IMPORT_PROMPT.md`：核验并接管已有任务交接。
- `CODEX_HANDOFF_UPDATE_PROMPT.md`：继续工作后增量刷新交接状态。
- `CODEX_HANDOFF_TEMPLATE.md`：交接文件的字段、证据链和完整性模板。
- `vma-unified/`：VMA 统一路径重构的分支任务拆解、验收与历史简报。

当前问题、性能实施和专项回归分别位于 `docs/tasks/known-issues/`、
`docs/tasks/perf/` 和 `docs/tasks/read-family/`。每项任务的已完成记录保留在其自身的
`history/` 或 `reports/` 目录。
