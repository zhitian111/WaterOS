# WaterOS 任务列表

本文件用于维护当前下发的 impl 任务、负责人和完成情况。更细的阶段目标请见 `docs/roadmap/todolist.md`。

## 状态说明

- `待办`
- `进行中`
- `Review 中`
- `已完成`

## 当前任务表

| 状态 | 类型 | 组件 | 任务 | 负责人 | 关联 |
|------|------|------|------|--------|------|
| 待办 | 实现 | wateros-fs | 完善 `impl-devfs` 的根目录与设备节点能力 | - | - |
| 待办 | 实现 | wateros-driver | 补充具体块设备实现并完善导出文档 | - | - |
| 已完成 | 实现 | wateros-ipc | 将 `ipc-pipe` 从占位实现推进到内核内部 ring-buffer pipe | Codex | `wateros-ipc` `pipe` feature |
| 已完成 | 实现 | wateros-syscall | 接入最小 pipe fd/syscall 与用户态 pipe smoke | Codex | `pipe2/read/write/close` |
| 已完成 | 实现 | wateros-task | 补最小父子关系与 child-exit 等待供 `waitpid` 使用 | Codex | `TaskWaitTarget::ChildExit` |
| 待办 | 设计 | wateros-mm | 继续收敛地址空间与映射接口 | 核心开发 | - |
| 待办 | 文档 | docs | 持续刷新 `exports/` 下的组件快照 | 核心开发 | - |

## 使用规则

- 新任务优先写清组件、目标和依赖。
- 认领任务后同步填写分支名或 PR 链接。
- 任务完成后同时更新 `docs/roadmap/todolist.md` 与相关导出文档。
