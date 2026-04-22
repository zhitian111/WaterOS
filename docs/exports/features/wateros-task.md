# wateros-task 功能快照

## 当前状态

当前已具备单核内核态任务切换、timer 驱动 round-robin 调度，以及 Stage3A 第一轮边界收紧后的任务/runtime/scheduler 分层。

当前已落地的能力包括：

- 任务对象由 `task-impl/impl-dummy` 统一承载
- `TaskSnapshot` 已收敛为稳定公共快照，不再暴露栈顶地址和启动协议细节
- 任务状态已从单纯 `Ready/Running` 扩展为 `Ready`、`Running`、`Blocking`、`Sleeping`、`Exited`
- 调度器已可区分 `yield`、timer tick、阻塞、睡眠与退出等调度原因
- 调度器已开始收敛为“任务注册表 + TaskId 队列”，并具备最小的阻塞队列、睡眠队列、退出队列和显式唤醒入口
- 已具备最小 `WaitQueue` 能力，可显式 `wait_current`、`wait_current_for_ticks`、`wake_one`、`wake_all`
- 已具备最小的 timed wait 与退出回收入口，可显式 `reap_exited_task`、`reap_one_exited_task`
- 已引入通用 `TaskWaitHandle` / `TaskWaitTarget`，`waitqueue` 与“等待任务退出”已共用同一条等待与 timeout 路径
- 退出任务现在会保留为可回收 zombie，并在退出时自动唤醒等待其退出的 waiter
- task 根 crate 已收紧为 facade，trap/tick/task-entry hook 已迁入内部 runtime
- trap 路径已开始把完整 trap frame 快照复制进当前任务对象，并在返回前回写到 trap 栈帧
- `current_task_snapshot` 可提供不含任务切换上下文、但包含最近一次 trap frame 快照的轻量任务状态快照与统计信息

## 后续关注点

- 继续把当前“复制 + 回写”模式推进为完整 trap frame 归属与恢复模型
- 继续把当前 wait handle 模型推进为更完整的通用阻塞对象 / block object 层
- 继续补更明确的 task handle / generation 语义，以及更贴近 `waitpid` 的上层回收关系
- 为用户态任务、syscall 返回路径和更复杂的 waitqueue 使用场景预留更稳定的接入面
- 持续补齐注释与公共 API 文档
