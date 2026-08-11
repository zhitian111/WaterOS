# WaterOS 文档总览

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

1. 先看 `agents/README.md` 和 `guides/workflow.md`。
2. 需要执行项目任务时查看 `tasks/README.md`。
3. 需要分析性能候选或查看个人待办时查看 `todo/README.md`。
5. 需要做内存管理验证时查看 `guides/mm-validation.md`。
6. 需要了解当前文件系统 bring-up 栈时查看 `guides/filesystem-current.md`。
6. 需要了解设备树、virtio 与 devfs 协作时查看 `guides/device-driver.md`。
7. 需要了解进程凭证（cred）模块设计与 BusyBox identity syscall 方案时查看 `guides/cred-module-design.md`。
8. 需要构建、运行、调试或性能采样时先查看 `workflows/README.md`，再按需查阅 `tools/README.md`。
