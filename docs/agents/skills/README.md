# Agent Skills

[项目首页](../../../README.md) · [文档总览](../../README.md) · [Agent 文档](../README.md) · [任务提示词](../tasks/README.md)

本目录存放可由 Agent 按任务需要加载的专项能力说明。Skill 描述一套可复用的判断规则、
执行流程和交付要求，适合需要稳定工作方法的任务；它不能替代用户指令、当前源码、
`AGENTS.md` 或项目技术规范。

## 技能索引

| Skill | 适用场景 | 主要作用 |
|:--|:--|:--|
| [`humanizer.md`](./humanizer.md) | 文案润色、审校和去除机械化表达 | 改善语言自然度，同时保留原意、事实与技术精度 |
| [`CODEX_VERIFIED_HANDOFF_SKILL.md`](./CODEX_VERIFIED_HANDOFF_SKILL.md) | 跨对话、工作树、主机或 Agent 交接任务 | 导出、接管或更新带有仓库状态和验证证据的任务现场 |

## 使用方式

开始任务前，根据用户请求选择最小必要的 Skill，并完整阅读相应文件。若 Skill 引用了
模板、任务提示词或其他资料，还应按其中的路径继续读取。多个 Skill 同时适用时，应先
确定执行顺序，避免流程之间相互覆盖。

例如，可验证任务交接还需要配合以下文件：

- [`CODEX_HANDOFF_EXPORT_PROMPT.md`](../tasks/CODEX_HANDOFF_EXPORT_PROMPT.md)：生成交接；
- [`CODEX_HANDOFF_IMPORT_PROMPT.md`](../tasks/CODEX_HANDOFF_IMPORT_PROMPT.md)：核验并接管交接；
- [`CODEX_HANDOFF_UPDATE_PROMPT.md`](../tasks/CODEX_HANDOFF_UPDATE_PROMPT.md)：增量更新交接；
- [`CODEX_HANDOFF_TEMPLATE.md`](../tasks/CODEX_HANDOFF_TEMPLATE.md)：交接文件结构。

## 与其他 Agent 文档的关系

- `prompts/` 提供长期项目背景、编码规范和架构约束；
- `tasks/` 提供可以直接下发的具体任务说明；
- `skills/` 提供跨任务复用的专项工作方法。

执行具体工作时，通常先读取适用的项目背景，再读取任务说明，最后加载必要的 Skill。
出现冲突时，以用户最新明确要求和当前仓库事实为准，其次遵守当前作用域内的
`AGENTS.md` 与项目规范。

## 新增与维护

- 一个 Skill 只解决一类清晰、可复用的问题；
- 文件名应稳定且能够体现用途；
- 文件开头应说明适用场景、输入、输出和关键限制；
- 涉及模板或配套提示词时使用仓库相对路径，并同步更新本索引；
- 不在 Skill 中写入 token、Cookie、私钥、账号或其他敏感信息；
- 当流程与当前代码、工具或目录结构不一致时，应及时修正文档漂移。
