# WaterOS 文档总览

本目录用于维护 WaterOS 的长期文档体系，分为五个层级：

- `prompts/`：提供给 Agent 的长期上下文、规则和工作流。
- `tasks/`：定义可周期执行的项目级重复任务。
- `exports/`：存放按任务导出的结果文档。
- `guides/`：提供给人阅读的规范、流程和协作说明。
- `roadmap/`：维护阶段目标和 TodoList。
- `architecture/`：维护架构快照与整体说明。

当前仓库仍保留旧版文档，例如 `WORKFLOW.md`、`COMMIT_CONVENTION.md`、`TASKS.md`、`KERNEL_INTERFACE_TODOLIST.md`。新体系优先使用本目录下的新结构，旧文件作为迁移期间的历史参考。

## 推荐阅读顺序

1. 先看 `prompts/README.md` 和 `guides/workflow.md`。
2. 再看 `architecture/snapshot.md` 了解项目结构。
3. 需要执行全项目任务时查看 `tasks/README.md`。
4. 需要了解当前系统状态时查看 `exports/`。
5. 需要做内存管理验证时查看 `guides/mm-validation.md`。
6. 需要了解当前文件系统 bring-up 栈时查看 `guides/filesystem-current.md`。
6. 需要了解设备树、virtio 与 devfs 协作时查看 `guides/device-driver.md`。
7. 需要了解进程凭证（cred）模块设计与 BusyBox identity syscall 方案时查看 `guides/cred-module-design.md`。
