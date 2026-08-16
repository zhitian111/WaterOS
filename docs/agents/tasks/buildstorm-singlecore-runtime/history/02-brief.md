# BS-SC-02 任务简报

## 状态

接受，批次 A 完整功能与性能验收待 BS-SC-04。commit 为本简报所在提交，完整 hash 由下一
任务补记。

## 修改与影响面

- task impl-core 新增 `process_task_context`，在一次关中断的 registry 临界区内取得
  `ProcessTaskSnapshot` 与 `ProcessState`；
- task 组合层新增 `CurrentProcessContext`，不修改 `task-api/api-v0`；
- signal delivery 的 Running 快路径复用该上下文建立 signal state，Stopped 分支在
  `block_current` 返回后重新查询，SIGKILL 不再重复取 current task id；
- trap 入口退出检查改用组合快照，普通返回和已完成 signal delivery 的提前返回删除重复
  检查；invalid rt_sigreturn、COW 与 lazy fault 三条未递送信号的路径保留显式检查。

CodeGraph 展开了 `finish_trap_return -> exit_current_if_process_exiting`、
`return_to_user_signal_delivery -> deliver_pending_signal`、`current_process_snapshot ->
current_process_task_snapshot -> process_task_snapshot` 以及 58-symbol 的
`process_task_snapshot` impact。状态所有权仍在 task impl-core，signal 层只消费复制快照。

## 查询次数与语义

普通 syscall 返回改前为 7 次 process-registry 临界区：入口退出检查 2 次、返回前重复退出
检查 2 次、signal delivery 的 process 状态与 signal descriptor 共 3 次。改后为 2 次：
入口组合查询 1 次，syscall/阻塞完成后 delivery 重新查询 1 次。后一个快照不会跨可能阻塞的
syscall 复用。

Stopped 分支仍先消费 SIGKILL，再阻塞；阻塞恢复后重新获取 task/pid/tid。Exiting/Exited
仍立即使用原 exit code 结束当前线程；Running 才进入 pending signal 处理。signal restart、
stop/continue 通知和 handler frame 逻辑未改。

## 实际验收

```text
make rv_check                         PASS（仅有既有 warning）
make la_check                         PASS（仅有既有 warning）
make kernel-rv-final                  PASS（release，4.53s）
make kernel-la-final                  PASS（release，7.11s）
git diff --check                      PASS
```

为组合查询新增 registry 单元测试，覆盖 Running、Stopped 和不存在 task。直接执行 impl-core
`cargo test` 时，组件 workspace 未为 `wateros-platform-arch` 选择实现，既有
`ArchPagingImpl` 无法解析，因此该 host 测试未运行；真实 RV/LA feature 组合的 check/build
均通过。不得把这个限制记为测试通过。

## 决定与剩余风险

保留该低风险提交。SIGKILL、SIGSTOP/SIGCONT、阻塞 syscall、rt_sigreturn 和普通 syscall 的
guest smoke，以及两架构完整性能结果统一由 BS-SC-04 验收。新增组合层 API 已有源码文档，
未改变稳定 `api-v0`、架构能力、feature 或用户构建接口，无其它文档同步项。
