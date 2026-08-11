# setpriority/sched nice 调用链快速路径（2026-08-11）

## 为什么选择这里

完整 pc-hot 采样中：

- `with_scheduler(set_nice)`：约 1.17B 指令；
- `with_scheduler(get_nice)`：约 0.22B 指令；
- `TaskRegistry::task_snapshot` 在 aspace 缓存优化后仍是高占比快照路径。

BuildStorm 本身不会频繁改变 nice，但 glibc/rustc/cargo 会调用
`setpriority(2)`/`getpriority(2)`。当前内核把一次很简单的 nice 查询/更新做成了
两条重复调用链：

```text
task::set_nice
  -> ensure_task_exists
      -> scheduler::task_snapshot      # 第一次取完整 TaskSnapshot + scheduler 锁
  -> scheduler::set_nice
      -> with_scheduler                # 第二次 scheduler 锁
      -> state/running_cpu/registry    # 再次查任务并写 TCB

task::get_nice
  -> ensure_task_exists
      -> scheduler::task_snapshot      # 完整快照 + scheduler 锁
  -> scheduler::get_nice
      -> with_scheduler                # 第二次 scheduler 锁
      -> task_snapshot                 # 再取一次完整快照
```

## 选择方案

1. 为 `TaskRegistry` 增加轻量 `nice(task_id) -> Option<i8>`，只读 TCB 中的
   `nice` 字段，不构造完整 `TaskSnapshot`。
2. `MultiClassScheduler::get_nice` 改为返回 `Result`，任务不存在时返回
   `NoSuchTask`，不再在缺失任务时 panic。
3. `task::set_nice` / `task::get_nice` 去掉 `ensure_task_exists`，直接进入
   `scheduler`；存在性由调度器层单次检查。
4. `MultiClassScheduler::set_nice` 在调度器锁内先比较旧 nice；如果目标值不变，
   直接成功返回，避免更新 CPU 热路径 cache 和 TCB。

## 为什么这么做

这是对内核调用路径本身的优化，不是替换 compiler-builtin 或基础库实现。目标是把
一次 nice 操作从“两次 scheduler 锁 + 两次完整快照”降为“一次 scheduler 锁 + 一次
轻量字段读”，并消除无变化写入。语义保持 Linux 的线程级 nice 行为不变。

## 接下来怎么做

1. 在 `perf/sched-nice-fastpath` 分支实现上述四项。
2. 双架构 `make check`，补/跑调度相关定向测试。
3. 完整 RISC-V BuildStorm 对照 main 约 `809.4s`。
4. 有效则记录 pc-hot 与耗时并合并；无效则回退并记录。

## 当前验证状态

- 实现完成后，`make rv_check`、`make la_check` 和 `make kernel-rv-final` 均通过；
  输出只有仓库已有的未使用项警告。
- `sched-nice-full-a1` 被用户中断，没有可验收结果；
  `sched-nice-full-a2` 因沙箱内 `/var/tmp` 只读导致 QEMU 未启动，也不计入性能样本。
- 两轮使用同一内核和同一只读 `-snapshot` 镜像的有效完整 BuildStorm 均功能通过：

  | run id | guest elapsed | 相对 main 中位数 809.42s |
  | --- | ---: | ---: |
  | `sched-nice-full-a3` | 866.88s | +7.10% |
  | `sched-nice-full-a4` | 879.77s | +8.69% |
  | 两轮中位数 | 873.33s | +7.90% |

- `sched-nice-pchot-a1` 按预期在固定 300s 窗口结束（302.316s，timeout，
  无 panic/stall）。`set_nice`、`get_nice` 和对应 `with_scheduler` 实例已不在
  top-80，且聚合符号表中没有匹配项，证明目标重复快照热点确实被移除。
  采样期主要热点仍是 `memcpy`、`memset`、`memcmp`、virtio 同步等待、TLSF
  分配/释放、路径规范化和用户内存复制。

## 验收结论

**否决，不合并代码。** 该方案局部上成功消除了 nice 调用链热点，但两个完整样本
均出现明显回退，且两轮中位数比主线慢 7.90%，不满足“至少 1.5% 且可重复”的
有效优化门槛。局部指令热点消失不能替代端到端验收；本分支仅保留实验文档，代码
恢复到分支起点。后续不要重复“只缩短 nice 快照链”的方案，除非新的调用频率证据
证明它重新成为端到端瓶颈。
