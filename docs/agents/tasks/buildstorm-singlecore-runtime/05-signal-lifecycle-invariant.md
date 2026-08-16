# BS-SC-05：建立 signal state 生命周期不变量

## 任务内容

将用户任务 signal state 的存在性从“每次用户返回懒 ensure”改为任务生命周期不变量，
使正常返回可以直接查询 deliverable signal，去掉一次 process registry 和一次 signal
registry ensure。

## 实施方案

1. 盘点初始用户任务、spawn、fork、clone thread、exec、thread exit、process exit、reap 和
   所有失败回滚路径。
2. 在用户任务对外可运行之前完成 signal process/thread 注册；发布 runnable 和 signal
   注册之间不得存在可被发送信号观察到的窗口。
3. fork/clone 失败时撤销 signal state；exec 保留进程 disposition/pending 的 Linux 语义，
   删除 sibling thread state；exit/reap 幂等清理。
4. `deliver_pending_signal` 改为依赖已注册状态，不再调用 `ensure_current_signal_state()`。
   debug/self-test 可断言不变量，release 热路径不得静默修复缺失状态。
5. syscall 主动创建目标进程状态的慢路径仍可保留 `ensure_process_signal_state`，但必须说明用途。
6. 尽量不改 signal `api-v0`；若现有契约确实不足，先在简报中列出所有实现和调用者。

## 涉及文件

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/signal.rs`
- `os/components/wateros-task/src/{spawn,lifecycle,process}.rs`
- `os/components/wateros-ipc/ipc-signal/signal-impl/impl-core/`
- 相关 fork/clone/exec/exit syscall 组合层
- `os/scripts/perf/guest-probes/signal_lifecycle.sh`（若无等价现有 probe）
- `history/05-brief.md`

## CodeGraph 查询

```bash
codegraph explore "ensure_current_signal_state on_fork on_clone_thread on_exec on_thread_exit"
codegraph callers "ensure_current_signal_state"
codegraph impact "fork_process"
codegraph impact "register_thread"
codegraph explore "spawn user task signal register runnable publication"
```

## 验收方式

本任务属于 signal 批次 B，只跑最窄检查，完整回归由任务 07 执行。

```bash
cd /home/zhitian/project/WaterOS_buildstorm_singlecore/os
make rv_check
make la_check
make kernel-rv-final
make kernel-la-final
cd ..
git diff --check
```

定向 probe 至少循环 10,000 次 fork/exit 与 pthread create/join，并混合 exec、kill、线程定向
signal；结束后 signal/process/task registry 数量回落，且没有 ESRCH 假失败或 signal state 泄漏。

## 完成后简报

新增 `history/05-brief.md`，列出所有注册/清理/回滚入口、热路径删除的锁、定向结果，并标记
“批次 B 完整验收待任务 07”。
