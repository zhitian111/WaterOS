# 任务 04：把 /proc IO 计数移入进程原子状态

## 任务内容与目标

消除每次成功 read/write 为 `/proc/<pid>/io` 进入 cwd registry 全局锁和多棵 BTreeMap 的
成本。四个字符 I/O 计数属于进程，线程共享；procfs 只在读取时快照原子值。

## 实施方案

1. 在进程核心状态增加 `rchar/wchar/syscr/syscw` 原子计数，使用饱和更新或明确 wrap 策略。
2. syscall 成功返回路径直接按当前进程引用累加，不查 cwd owner 表。
3. fork 初始化、线程 clone 共享、exec 保留、reap 销毁语义写入注释和测试。
4. 删除 cwd registry 的 `io_counters` 及兼容转发，procfs callback 改读进程快照。

## 涉及文件

- `os/components/wateros-task/task-impl/impl-core` 的进程状态模块
- `os/components/wateros-task/src/process.rs` 或窄 facade
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/lib.rs`
- `os/components/wateros-vfs/src/cwd.rs` 与 `impl-fd-session/src/cwd.rs`
- `os/components/wateros-fs/fs-procfs/procfs-impl/impl-kernel/src/render.rs`

## CodeGraph 查询

```bash
codegraph explore "account_task_io ProcessIoCounters current_process_snapshot"
codegraph callers "account_task_io"
codegraph impact "ProcessIoCounters"
```

## 验收方式

```bash
cd os
make rv_check && make la_check && make kernel-rv-final
# 运行 proc/self/io、线程共享、fork/exec 定向用户回归
cd .. && git diff --check
```

计数值与改动前相同；任务 01 的 cwd registry I/O-account 进入次数降为零。任务 00 runner
完成 A/B，确认 read/write 密集阶段无回退。

## Commit 与简报

提交建议：`[perf] 进程对象使用原子 IO 计数`。新增 `history/04-brief.md`。
