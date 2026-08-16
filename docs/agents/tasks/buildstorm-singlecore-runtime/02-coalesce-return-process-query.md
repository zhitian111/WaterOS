# BS-SC-02：合并用户返回的重复进程状态查询

## 任务内容

普通用户 trap 返回先执行 `exit_current_if_process_exiting()`，紧接着
`deliver_pending_signal()` 再查询并处理 `Stopped/Exiting/Exited`。移除返回路径重复检查，
并让信号递送在一次 process-registry 临界区获得 task/pid/tid/state 上下文。

## 实施方案

1. 保留 trap 入口的退出检查，先只删除普通返回和 `finish_trap_return` 中被信号递送覆盖的
   重复检查；逐个核对提前返回分支。
2. 在 task 实现内部增加窄的 `CurrentProcessContext`/等价快照，一次 registry 锁返回
   `task_id/pid/tid/state`；不修改公共 `api-v0`。
3. `deliver_pending_signal` 接收或复用该上下文，避免 `current_process_snapshot()` 的
   task→pid、pid→process 两段查询。
4. 阻塞或调度后不得继续使用可能过期的 `ProcessState`；恢复后需要状态时重新取快照。
5. 不改变 stopped/exiting/exited 的处理顺序、退出码和 signal restart 语义。

## 涉及文件

- `os/src/trap_handler.rs`
- `os/components/wateros-task/src/process.rs`
- `os/components/wateros-task/task-impl/impl-core/` 中 process registry 的语义所有文件
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/signal.rs`
- `history/02-brief.md`

## CodeGraph 查询

```bash
codegraph explore "exit_current_if_process_exiting deliver_pending_signal current_process_snapshot"
codegraph callers "current_process_snapshot"
codegraph impact "process_task_snapshot"
codegraph explore "ProcessRegistry process_task_snapshot process_snapshot"
```

## 验收方式

本提交属于低风险批次 A，不单独跑完整 BuildStorm。

```bash
cd /home/zhitian/project/WaterOS_buildstorm_singlecore/os
make rv_check
make la_check
make kernel-rv-final
make kernel-la-final
cd ..
git diff --check
```

定向 smoke 至少覆盖：正常 syscall 返回、退出中的线程、SIGKILL、SIGSTOP/SIGCONT、阻塞 syscall
被信号打断以及 `rt_sigreturn`。批次完整验收由任务 04 执行。

## 完成后简报

新增 `history/02-brief.md`，给出改前/改后普通返回路径的 process-registry 查询次数、状态语义
核对结果，并标记“批次 A 完整验收待任务 04”。
