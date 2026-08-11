# K-06C Process exit/reap 生命周期报告（2026-08-01）

## 问题与根因

初赛镜像按 `epoll-ltp -> exit_group01` 执行时会随机停在 `exit_group01` 或其父 shell：

1. `exit_group` 在发布 `ProcessState::Exiting` 前向远端 CPU 发重调度通知，目标线程可能
   消费通知后仍观察到 `Running`，之后没有第二次通知。
2. 多个线程分别执行“是否最后线程”查询和退出标记，可能都先判断为否；最终 PCB 已为
   `Exited`，却没有线程负责唤醒父 `waitpid`。
3. process reap 在 Registry 借用期间销毁地址空间，扩大关中断临界区和锁序风险。

## 实现

- `begin_current_process_exit` 在通知兄弟线程前发布 `Exiting`。
- `ProcessRegistry::mark_task_exited` 在同一临界区内更新线程状态并返回 PCB 是否刚转为
  `Exited`；完成者负责信号表最终清理、`SIGCHLD` 和 parent wait 唤醒。
- 删除会诱导 TOCTOU 的 `task_exit_would_finish_process` 查询接口。
- 引入 owned `RetiredProcess`：Registry 锁内 detach，锁外销毁用户地址空间；fork
  回滚复用相同两阶段模型。

## 验证

- `make rv_check`、`make la_check`：通过（仅有既有 warning）。
- RISC-V64/OpenSBI/8 CPU，初赛镜像原生 `epoll-ltp`（13,824 项）后接原生
  `exit_group01`：修复前 3 轮中 1 轮在 LTP 输出后父 shell 永久等待；修复后连续
  3 轮均 `EPOLL_LTP_RC=0`、`EXIT_GROUP_RC=0`，命令约 67--69 秒正常结束。
- 测试只临时收窄 bringup 命令；仓库未保留自制测试或测试日志。

完整 LTP、10,000 次 fork/exit 资源有界性及决赛回归由 K-10 继续验收。
