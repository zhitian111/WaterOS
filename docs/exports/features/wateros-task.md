# wateros-task 功能快照

## 当前状态

当前已具备单核内核态任务切换、timer 驱动 round-robin 调度，以及第二阶段最小生命周期语义。

当前已落地的能力包括：

- 任务对象由 `task-impl/impl-dummy` 统一承载
- 任务状态已从单纯 `Ready/Running` 扩展为 `Ready`、`Running`、`Blocking`、`Sleeping`、`Exited`
- 调度器已可区分 `yield`、timer tick、阻塞、睡眠与退出等调度原因
- 已具备最小的阻塞队列、睡眠队列、退出队列和显式唤醒入口
- `current_task_snapshot` 可提供当前任务的轻量状态快照与统计信息

## 后续关注点

- 继续把 trap frame 归属关系并入任务对象
- 为用户态任务、syscall 返回路径和 waitqueue 预留更稳定的接入面
- 持续补齐注释与公共 API 文档
