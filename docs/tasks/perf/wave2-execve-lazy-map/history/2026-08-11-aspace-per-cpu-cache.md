# per-CPU 当前用户地址空间指针缓存（2026-08-11）

## 为什么选择这里

最近两轮完整 pc-hot 中，`copy_from_user` 仍约 `530M-544M` 条指令，scheduler 快照相关
符号也继续出现在 Top 热点：

```text
current_task_user_aspace_ptr
  -> scheduler::current_task_snapshot
     -> with_scheduler
        -> TaskRegistry::task_snapshot + live tick/vruntime 补算
```

用户复制的公共入口只为了拿到当前任务的 `user_aspace_ptr` 一个字段，却每次构造完整
`TaskSnapshot` 并访问全局 scheduler 锁。Linux 的 user access 只依赖当前 mm 的轻量
per-CPU 指针，不会每次 syscall 都重建调度快照。

## 优化方案

1. 在 scheduler impl 增加 `CURRENT_ASPACE_PTRS : [AtomicUsize; MAX_CPUS]`。
2. 在 `with_scheduler` 的统一尾部，与 `CURRENT_TASK_IDS` 一起发布
   `scheduler.cpu_states[cpu_id].current_aspace()`。
3. `current_task_user_aspace_ptr()` 改为关闭本地中断后读取当前 CPU 的
   `CURRENT_ASPACE_PTRS`；0 表示 kernel/idle/无用户地址空间，与 `TaskSnapshot`
   的语义一致。
4. 不改变 `current_task_snapshot()` 本身；其它仍需要完整快照的调用方保留原路径。
5. 不新增 `api-v0` 稳定接口；只替换 task 聚合层的当前地址空间便捷查询。

## 为什么先前的 COPY-02A 失败

上一版尝试在 `with_scheduler` 尾部发布 aspace，但完整 BuildStorm 超时。可能原因是实现
把发布时机依赖在“上下文切换后返回统一尾部”，而当时的验证没有覆盖 exec 后首个用户
访问。本轮要求：

- 发布点必须覆盖所有 `set_current_task`、`set_current_aspace`、`execve_current` 路径；
- `current_task_user_aspace_ptr` 读取时关闭本地中断，避免读槽位时被切走；
- 增加“切换后首个 syscall 返回正确 aspace”的双架构运行回归；
- 若完整轮仍超时，立即回退并保留记录，不重复叠加其它改动。

## 下一步

1. 实现 `CURRENT_ASPACE_PTRS` 并接入 `with_scheduler` 尾部。
2. 双架构 Final check/build，跑 Final smoke 覆盖 exec/fork/copy_from_user。
3. 完整 BuildStorm A/B 与 300 秒 pc-hot A/B。
4. 有效则合并 main，无效则回退并记录。

## 验证结果

- 双架构 Final `make check` 通过。
- Final smoke 通过：根卷、VFS 自检、cagent 全部通过，并进入 BuildStorm。
- 完整 BuildStorm 两轮：
  - `BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=808.90`
  - `BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=809.94`
  - 中位数约 `809.42s`，相对 main `817.27s` 快约 `0.96%`。
- 300 秒 pc-hot A/B：
  - `copy_from_user`：约 `528.7M` -> `463.0M`（-12.4%）；
  - `TaskRegistry::task_snapshot`：约 `166.7M` -> `50.5M`（-69.7%）；
  - 总指令 top `??` 从约 `10.72B` 降到 `9.00B`，与移除 scheduler 快照的预期一致。

## 结论

完整 BuildStorm 收益接近运行噪声边界，但两轮结果一致且 pc-hot 证明热路径确实减少，
因此保留并合并到 main。
