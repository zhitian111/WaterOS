# ProcessRegistry per-parent child/exited 索引（2026-08-11）

## 为什么选择这里

你观察到 BuildStorm 在 `ax-posix-api`/`rustc-std-workspace-core` 后越来越慢。代码分析
发现一个非常符合这个现象的 O(N) 共性热点：`waitpid/waitid` 在每次查询 exited child 时，
都会全量扫描 `ProcessRegistry.processes`：

```text
wait4/waitpid
  -> find_exited_child_process
     -> processes.values().find(parent_pid == self && Exited)
```

Cargo/shell 会频繁创建和等待大量子进程；进程表里同时存在的 Running/Exited 进程越多，
每次 wait 扫描越长，于是越到编译后期越慢。`collect_child_pids`、`has_child_process` 也
同样线性扫描。

## 优化方案

在 `ProcessRegistry` 增加两个 per-parent 索引：

1. `children: BTreeMap<ProcessId, BTreeMap<ProcessId, ProcessId>>`：记录每个父进程的
   子进程集合，用于 `has_child_process` 和 `collect_child_pids`。
2. `exited_children: BTreeMap<ProcessId, VecDeque<ProcessId>>`：记录每个父进程下
   已退出、尚未 reap 的子进程，用于 `find_exited_child_process`。

索引生命周期：

- `create_process_for_task`：注册到父进程 children；
- `mark_process_exited`：子进程进入 Exited 后入队到当前父进程 exited_children；
- `reparent_orphans`：先移除旧父进程索引，再注册到新父进程，并同步 exited 状态；
- `remove_process` / reap：从旧父进程 children 和 exited_children 中移除。

## 为什么这么做

这是把 wait 热路径从“每轮扫描全部进程”降为“只访问该父进程子集”的结构性优化，不改
wait 语义，也不触碰 `api-v0`。后续 BuildStorm 越到后期，收益越明显，符合“共性问题
优先”的方向。

## 下一步

1. 实现索引并同步所有生命周期路径。
2. 双架构 Final check/build，跑 fork/wait/reparent 定向测试。
3. 完整 BuildStorm A/B 与 300 秒 pc-hot A/B。
4. 有效则合并 main，无效则回退并记录。

## 验证结果：回退

实现完成后先通过 RISC-V/LoongArch Final check，并跑了一轮完整 RISC-V BuildStorm：

```text
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=860.46 cores=8 bytes=1681000 arch=riscv64
host_wall_s=902.011
```

对照同一台机器、同一镜像上的 main 中位数约 `809.4s`，本轮明显慢约 `6.3%`，没有达到
“至少与 main 持平”的保留门槛，因此回退代码改动，不合并。

结论：

- waitpid/waitid 每次全量扫描 `ProcessRegistry.processes` 这个假设在 BuildStorm 实际
  负载中没有带来可测收益；
- 每次 fork/exit/reap/reparent 额外维护两个索引，反而增加了主路径开销；
- 后续不再沿着“给 ProcessRegistry 加 per-parent 索引”的方向继续，除非先拿到能证明
  wait 扫描真正占主导的调用计数。
