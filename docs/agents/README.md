# Agent 文档索引

[项目首页](../../README.md) · [文档总览](../README.md) · [任务索引](../tasks/README.md)

本目录集中存放供 Agent 复用的上下文、任务提示词与技能说明。按任务组合所需资料，
不要只提供单一文件。

## 目录

- [`prompts/`](./prompts/)：长期上下文与通用规则。
  - `general.md`：总则、交付形式与默认工作流。
  - `structure.md`：项目结构、关键目录与同步文件。
  - `coding.md`、`documentation.md`、`architecture.md`：编码、文档和架构约束。
  - `debug_workflow.md`、`update_docs_task.md`：专项调试与文档维护上下文。
- [`tasks/`](./tasks/README.md)：可直接下发的通用任务提示词，包括审计、导出、日志分析、
  LTP、QEMU 测试，以及任务交接的生成、接管、更新提示词和标准模板。
- [`skills/`](./skills/README.md)：可按需装载的专项能力说明与使用索引。
  - `humanizer.md`：用于文字编辑与审校，不应替代项目技术规范；
  - `CODEX_VERIFIED_HANDOFF_SKILL.md`：用于跨对话、工作树、主机或 Agent 保存并核验任务现场。

## 使用建议

向 Agent 提供本目录的任务提示词即可（如 `@docs/agents/tasks/commenting.md`）；任务文件内已列出须阅读的上下文与完整路径。下表仅为速查。

- 编码任务：至少阅读 `general.md`、`structure.md`、`coding.md`、`architecture.md`；再按任务定位相关源码和当前任务记录。
- 文档任务：至少阅读 `general.md`、`structure.md`、`documentation.md`、`architecture.md`。
- 代码注释专项（`docs/agents/tasks/commenting.md`）：除上述外，严格按该任务文件的**搜索范围**执行——覆盖 **`os/components/**` 全子树**、**`os/src/`**、**`user/**`** 等，**不得**仅处理一级聚合 crate 或默认 feature 路径；细则见 `documentation.md` 中「覆盖范围」。
- 规划任务：至少阅读 `general.md`、`structure.md`、`architecture.md`。
- **编译 / QEMU 运行 / 测例回归**：阅读 `general.md`「构建与运行」、`coding.md` §6，以及 `docs/agents/tasks/run_testsuits_qemu.md`。
- **日志分析**（`docs/agents/tasks/analyze_kernel_log.md`）：阅读 `general.md`、`structure.md`、`architecture.md`，并按失败子系统选读相关源码。
- 对文档体系本身做修改：额外阅读 `update_docs_task.md`。
- **任务交接**：阅读 `skills/CODEX_VERIFIED_HANDOFF_SKILL.md`，并按交接方向使用
  `tasks/CODEX_HANDOFF_{EXPORT,IMPORT,UPDATE}_PROMPT.md`；交接文件结构以
  `tasks/CODEX_HANDOFF_TEMPLATE.md` 为准。
