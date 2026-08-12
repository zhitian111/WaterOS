<div align="center">
  <a href="../README.md">
    <img src="./assert/cover.jpg" height="72" alt="山东大学" />
  </a>
  <h1>WaterOS 文档</h1>
  <p>架构说明、开发工具、标准流程与项目记录</p>
  <p>
    <a href="../README.md">项目首页</a> ·
    <a href="../os/README.md">内核工程</a> ·
    <a href="./tools/README.md">工具文档</a> ·
    <a href="./workflows/README.md">标准流程</a>
  </p>
</div>

---

本目录用于维护 WaterOS 的长期文档体系：

- `agents/`：提供给 Agent 的长期上下文、可复用任务提示词与技能说明。
- `tasks/`：当前任务、历史记录和跨任务报告。
- `todo/`：尚未收敛的性能分析与队友维护的个人待办。
- `guides/`：提供给人阅读的规范、流程和协作说明。
- `tools/`：项目脚本、调试和测试工具的使用入口。
- `workflows/`：构建、调试、性能分析等可重复执行的标准流程。
- `technical_document/`：LaTeX 技术文档及其分章写作说明。

当前仓库仍保留旧版文档，例如 `WORKFLOW.md`、`COMMIT_CONVENTION.md`、`TASKS.md`、`KERNEL_INTERFACE_TODOLIST.md`。新体系优先使用本目录下的新结构，旧文件作为迁移期间的历史参考。

## 推荐阅读顺序

1. 构建、运行和调试内核时，先看 [`../os/README.md`](../os/README.md)。
2. 查找工具时，从 [`tools/README.md`](./tools/README.md) 进入。
3. 执行可重复操作时，遵循 [`workflows/README.md`](./workflows/README.md)。
4. 接手开发任务时，查看 [`tasks/README.md`](./tasks/README.md) 和对应交接记录。
5. 自动化 Agent 在工作前阅读 [`agents/README.md`](./agents/README.md)。
6. 需要项目技术文稿时进入 [`technical_document/README.md`](./technical_document/README.md)。

README 的分级页头、导航和事实引用约定见
[`guides/readme-style.md`](./guides/readme-style.md)。
