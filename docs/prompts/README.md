# Prompt 体系索引

本目录中的文档用于作为 Agent 的长期上下文。不同任务应组合引用不同 prompt，而不是只看单一文件。

## 文件索引

- `general.md`：总则，定义角色、交付形式、回答风格、默认工作流。
- `structure.md`：项目文本结构、关键目录、重要同步文件。
- `coding.md`：按文件角色组织的编码规范。
- `documentation.md`：广义文档规范，包括注释、提交和 PR。
- `architecture.md`：模块化架构、API/impl 范式和 feature 机制。
- `update_docs_task.md`：用于后续继续修改本套文档体系时的续写上下文。

## 使用建议

- 编码任务：至少阅读 `general.md`、`structure.md`、`coding.md`、`architecture.md`。
- 文档任务：至少阅读 `general.md`、`structure.md`、`documentation.md`、`architecture.md`。
- 代码注释专项（`docs/tasks/commenting.md`）：除上述外，严格按该任务文件的**搜索范围**执行——覆盖 **`os/components/**` 全子树**、**`os/src/`**、**`user/**`** 等，**不得**仅处理一级聚合 crate 或默认 feature 路径；细则见 `documentation.md` 中「覆盖范围」。
- 规划任务：至少阅读 `general.md`、`structure.md`、`architecture.md`。
- **编译 / QEMU 运行 / 测例回归**：阅读 `general.md`「构建与运行」、`coding.md` §6，以及 `docs/tasks/run_testsuits_qemu.md`。
- 对文档体系本身做修改：额外阅读 `update_docs_task.md`。
