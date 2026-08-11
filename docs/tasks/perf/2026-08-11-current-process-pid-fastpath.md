# 当前进程 PID 轻量查询（2026-08-11）

## 为什么选择这里

CodeGraph 调用方分析显示，`ProcessRegistry::process_task_snapshot` 是当前 pc-hot 中
约 `189M` 条指令的热点。大量 syscall 和 task 内部路径只是拿当前进程的 `pid`，却调用
`current_process_task_snapshot()`，构造包含 `tid/role/tls/clear_child_tid` 的完整
`ProcessTaskSnapshot`：

```text
truncate / flock / fcntl / rlimit / umask / prctl / getpriority
  -> current_process_task_snapshot()
     -> process_task_snapshot()
        -> pid_for_task BTree lookup
        -> processes BTree lookup
        -> ProcessTask::snapshot()
```

已有 `process_identity_for_task` 只返回 `(pid, parent_pid)`，正好可以覆盖这些调用方。

## 优化方案

1. 在 `wateros-task` 增加 `current_process_pid() -> Option<ProcessId>`，内部使用
   `process_identity_for_task` 的轻量查询。
2. 将明确只使用 `pid` 的调用方从 `current_process_task_snapshot().map(|s| s.pid)`
   改为 `current_process_pid()`。
3. 保留 `current_process_task_snapshot()`，供需要 `tid/role/tls/clear_child_tid`
   或进程级快照的真实路径使用。

## 为什么这么做

这比直接优化 `ProcessRegistry` 或 `memcmp` 更符合调用方驱动的思路：不改变语义，只避免
为只读一个字段而构造完整快照。风险低，且不触碰 `api-v0` 稳定契约。

## 下一步

1. 实现 `current_process_pid()` 并替换 pid-only 调用方。
2. 双架构 Final check/build，跑 fork/exec/rlimit/prctl 相关 smoke。
3. 完整 BuildStorm A/B 与 300 秒 pc-hot A/B。
4. 有效则合并 main，无效则回退并记录。

## 验证结果

- 双架构 Final `make check` 通过。
- Final smoke 通过：根卷、VFS 自检、cagent 全部通过，并进入 BuildStorm。
- 完整 BuildStorm：
  `BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=858.59`，相对当前 main
  `809.42s` 慢约 `6.07%`。

## 结论

调用方分析方向是对的，但当前 `process_identity_for_task` 仍是同一个进程 registry
全局锁下的 BTree 查询；替换 pid-only 调用方后没有减少主要成本，反而因为函数调用布局
和查询路径变化拖慢了完整轮。代码已全部回退，只保留本记录。

后续要真正优化进程身份查询，应先把当前进程 `pid` 做成 per-task 发布缓存，并只在
spawn/exec/exit 时失效，而不是让每个 syscall 都重复锁 registry 做两次 BTree 查询。
