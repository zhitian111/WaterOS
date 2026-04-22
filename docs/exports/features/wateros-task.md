# wateros-task 功能快照

## 当前状态

当前已具备单核内核态任务切换、timer 驱动 round-robin 调度，以及第二阶段最小生命周期语义。

当前已落地的能力包括：

- 任务对象由 `task-impl/impl-dummy` 统一承载
- 任务状态已从单纯 `Ready/Running` 扩展为 `Ready`、`Running`、`Blocking`、`Sleeping`、`Exited`
- 调度器已可区分 `yield`、timer tick、阻塞、睡眠与退出等调度原因
- 调度器已开始收敛为“中央 task 表 + TaskId 队列”，并具备最小的阻塞队列、睡眠队列、退出队列和显式唤醒入口
- 已具备最小 `WaitQueue` 能力，可显式 `wait_current`、`wake_one`、`wake_all`
- trap 路径已开始把完整 trap frame 快照复制进当前任务对象，并在返回前回写到 trap 栈帧
- `current_task_snapshot` 可提供不含任务切换上下文、但包含最近一次 trap frame 快照的轻量任务状态快照与统计信息

## 后续关注点

- 继续把当前“复制 + 回写”模式推进为完整 trap frame 归属与恢复模型
- 继续把最小 `WaitQueue` 推进为更通用的阻塞对象 / timeout 模型
- 为用户态任务、syscall 返回路径和更复杂的 waitqueue 使用场景预留更稳定的接入面
- 持续补齐注释与公共 API 文档
