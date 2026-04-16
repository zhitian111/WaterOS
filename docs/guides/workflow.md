# WaterOS 协作流程

本文件是当前协作流程的主入口，吸收旧版 `WORKFLOW.md` 和 `COMMIT_CONVENTION.md` 的内容，并结合当前文档体系重新组织。

## 角色分工

### 核心开发

- 维护组件边界与总体架构。
- 维护 `api-v0` 中的接口设计。
- 维护 feature 树和关键导出链。
- 评审实现侧 PR。

### 实现开发

- 在既定 API 下完成 `impl-*` 中的实现。
- 按要求更新 feature、依赖和聚合层。
- 通过分支和 PR 交付改动。

## 标准流程

1. 创建或认领任务。
2. 从最新主线创建分支。
3. 在对应组件内开发。
4. 本地完成最小验证。
5. 按规范提交并发起 PR。
6. 由核心开发 Review。
7. 合并后同步更新任务状态与文档。

## 任务分配方式

当前项目以“安排具体 impl 任务”为主要协作模式：

- 先由核心开发定义 API 边界。
- 再下发具体 impl 任务。
- 成员认领任务后创建分支实现。
- 完成后通过 PR 回到主线。

## 提交与 PR

- commit 使用 Conventional Commits。
- PR 标题使用同一风格。
- PR 描述需要说明涉及组件、与 API 的关系和测试方法。

## 与文档体系的关系

- 协作规则的事实来源在本文件。
- 任务清单维护在 `docs/guides/task-board.md`。
- 阶段性开发计划维护在 `docs/roadmap/todolist.md`。
- 架构和当前能力快照维护在 `docs/architecture/snapshot.md` 与 `docs/exports/`。
