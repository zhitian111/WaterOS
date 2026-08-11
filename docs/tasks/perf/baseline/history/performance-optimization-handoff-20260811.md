# WaterOS BuildStorm 性能优化交接（2026-08-11）

## 交接目标

继续把 WaterOS 的 BuildStorm 完整编译耗时推向 Linux baseline。当前已经跑通决赛
BuildStorm，性能优化仍是主任务；不要转向功能修复、文件系统新特性或架构重构，除非
它们是性能优化的必要前置。

当前主要指标：

| 指标 | 数值 |
|---|---:|
| Linux RISC-V baseline | 395.90s |
| WaterOS main 中位数 | 约 809.4s |
| 阶段一目标（2x baseline） | 约 791.8s |
| 最终目标（R <= 1） | 约 395.9s |

判定口径是同一宿主、同一 QEMU 参数、同一镜像、`-snapshot` 下的完整
`BUILDSTORM_COMPILE mode=multi ok=true elapsed_s`。短 pc-hot 只用于筛选，不用于验收。

## 当前 Git 状态

- `main` HEAD：`029dd86b`
- 当前分支：`perf/sched-nice-fastpath`
- 该分支上有未提交的 `setpriority/sched nice` 快速路径改动，尚未验证完：
  - `os/components/wateros-task/src/sched.rs`
  - `os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/registry.rs`
  - `os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/scheduler/policy.rs`
  - 方案文档：`docs/tasks/perf/2026-08-11-sched-nice-fastpath.md`
- 工作区里 `user` 子模块 dirty、大量未跟踪资料/镜像/文档属于用户，不要提交或清理。
- 本地 `main` 比 `github/main` 超前若干 commit；本交接不涉及 push。

## 已完成且保留的性能工作

- `[perf] cache current user aspace per cpu`
  - 文档：`docs/tasks/perf/2026-08-11-aspace-per-cpu-cache.md`
  - 让 user copy 不需要完整 `current_task_snapshot`，`copy_from_user` 和
    `TaskRegistry::task_snapshot` 均显著下降。
- `[perf] use single page TLB invalidation for COW faults`
  - 文档：`docs/tasks/perf/2026-08-11-cow-single-page-flush.md`
- `[perf] share cached ext4 inode snapshots`
  - 文档：`docs/tasks/perf/linux-baseline-optimization-log.md` 的 FS-04A 段。

## 已尝试并回退的实验

### process-child-index

- 文档：`docs/tasks/perf/2026-08-11-process-child-index.md`
- commit：`b40dc8d6`
- 完整轮：`elapsed_s=860.46`
- 结论：给 `ProcessRegistry` 增加 per-parent child/exited 索引反而变慢，已回退。

### u128 时间换算快速路径

- 文档：`docs/tasks/perf/2026-08-11-u128-time-conversion-fast-path.md`
- commit：`029dd86b`
- 完整轮：`848.96s`、`839.53s`
- 结论：`u128_div_rem` 指令确实下降，但完整轮仍明显慢于 main。用户明确要求不要把
  compiler-builtin 本身作为主要优化对象；后续应优化内核调用链，而不是替换基础数值实现。

### task lightweight runtime stats

- 文档只存在于旧分支，不在当前 `main` 工作区：
  `git show cc14dde6:docs/tasks/perf/2026-08-11-task-lightweight-runtime-stats.md`
- 完整轮：`809.22s`，但复跑停滞，不可验收；已回退。
- 教训：减少快照构造不足以改善完整轮，主要成本仍在 scheduler 全局锁/查询路径。

### current process pid fastpath

- 文档只在旧分支：
  `git show eec7f3b4:docs/tasks/perf/2026-08-11-current-process-pid-fastpath.md`
- 完整轮：`858.59s`
- 教训：pid-only 查询仍走同一进程 registry 全局锁和两次 BTree 查询；不能只把完整
  快照换成轻量查询。真正方向应减少锁获取或把 per-task 字段做成发布缓存。

## 当前未验证的 sched-nice-fastpath

实现内容：

1. `TaskRegistry` 新增轻量 `nice(task_id) -> Option<i8>`，不构造 `TaskSnapshot`。
2. `MultiClassScheduler::get_nice` 改为返回 `Result`，缺失任务返回 `NoSuchTask`。
3. `task::set_nice` / `task::get_nice` 去掉 `ensure_task_exists`，避免一次完整快照
   加一次 scheduler 锁后，又进入 scheduler 再拿一次锁。
4. `MultiClassScheduler::set_nice` 在锁内先比较旧 nice；相同值直接返回，避免更新
   CPU 热路径 cache 和 TCB。

验证状态：

- RISC-V Final `make check` 通过。
- LoongArch Final `make check` 通过。
- 完整 BuildStorm 在 `sched-nice-full-a1` 中跑到正式编译中段，随后被用户中断，没有
  可验收结果。

交接建议：

- 继续验证：先跑完整 RISC-V BuildStorm；若有效再跑 pc-hot 看 `set_nice/get_nice`
  是否下降；若无效或回归，按惯例回退并记录。
- 也可以先回退：这个改动不是既定结论，且用户提醒过“轻量 snapshot”类优化历史上
  没有改善完整轮。不要把它默认当成可合并优化。

## 用户约束与工作方式

1. 性能优化是唯一主任务；不要开展与 BuildStorm 性能无关的大改动。
2. 每个优化开始前先写方案文档：为什么选这里、方案是什么、为什么这样做、接下来怎么
   验证。
3. 每次优化完成后要做：双架构 check、完整 BuildStorm、pc-hot/需要时 wait-hot、文档
   归档、commit。
4. 可以互相隔离的实验放独立 `perf/...` 分支；有效再合回 main，无效回退并记录。
5. 优先优化内核调用路径，不要花主要精力优化 compiler-builtin、memcpy/memcmp 等基础
   符号本身。
6. 之前 “减少快照构造” 的实验多次没有完整轮收益；重新触碰时必须区分“快照复制成本”
   和“全局锁/BTree 查询成本”，并先证明主要成本在哪一层。
7. 用户有时白天禁止全量测试；以用户最新指示为准。允许夜间跑长测时再跑完整轮。
8. 所有完整轮使用 `-snapshot`，避免磁盘被前一轮写入污染。
9. 不要清理用户未跟踪文件：`.codegraph/`、`docs/`、镜像、内核产物等都属于用户工作区。

## 必读文档

先读：

- 根目录 `AGENTS.md`
- `os/AGENTS.md` 和 `os/AGENT.md`
- `docs/tasks/perf/2026-08-10-linux-baseline-optimization-roadmap.md`
- `docs/tasks/perf/heap-optimization-handoff-20260810.md`
- `docs/tasks/perf/waithot-full-analysis-20260807.md`
- `docs/tasks/perf/linux-baseline-optimization-log.md`
- `docs/tasks/perf/pc-hot-analysis-log.md`
- `docs/tasks/perf/2026-08-11-aspace-per-cpu-cache.md`
- `docs/tasks/perf/2026-08-11-process-child-index.md`
- `docs/tasks/perf/2026-08-11-u128-time-conversion-fast-path.md`
- 旧分支上的两条 rejected 文档：
  - `git show cc14dde6:docs/tasks/perf/2026-08-11-task-lightweight-runtime-stats.md`
  - `git show eec7f3b4:docs/tasks/perf/2026-08-11-current-process-pid-fastpath.md`

## 必读代码

- `os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/lib.rs`
  - `with_scheduler` 每次都会发布 current task/aspace/tick；查询类调用是否仍需要这
    份发布开销值得继续分析。
- `os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/scheduler/policy.rs`
  - `set_nice/get_nice/policy/priority`
- `os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/registry.rs`
  - `task_snapshot/state/nice/set_nice`
- `os/components/wateros-task/src/sched.rs`
  - `ensure_task_exists/resolve_sched_pid/set_nice/get_nice`
- `os/components/wateros-task/task-impl/impl-core/src/process.rs`
  - 已回退的 child index 不重做；可读 process snapshot 调用方。
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/mount_table.rs`
  - `mount_namespace_snapshot/resolve_route`
- `os/components/wateros-driver/driver-block/block-impl/impl-block-cache/src/lib.rs`
- `os/components/wateros-driver/driver-block/block-impl/impl-virtio-mmio/src/lib.rs`

## 性能工具

当前 runner 在临时 worktree：

```text
/tmp/wateros-perf-tools.waxdhM/os/scripts/perf/buildstorm_runner.py
```

如果该路径消失，可从 `perf/tlsf-slab` 分支重建：

```bash
TOOL_TREE=$(mktemp -d /tmp/wateros-perf-tools.XXXXXX)
git worktree add --detach "$TOOL_TREE" perf/tlsf-slab
ln -s /home/zhitian/project/WaterOS_refactor/final_test_case "$TOOL_TREE/final_test_case"
```

完整轮示例：

```bash
python3 /tmp/wateros-perf-tools.waxdhM/os/scripts/perf/buildstorm_runner.py \
  --arch rv \
  --kernel /home/zhitian/project/WaterOS_refactor/os/kernel-rv-final \
  --image /home/zhitian/project/WaterOS_refactor/os/sdcard-rv-pub.img \
  --run-id <exp>-full-a1 \
  --timeout 1200 \
  --output-root /home/zhitian/project/WaterOS_refactor/os/tem/perf/buildstorm
```

pc-hot 示例：

```bash
python3 /tmp/wateros-perf-tools.waxdhM/os/scripts/perf/buildstorm_runner.py \
  --arch rv \
  --kernel /home/zhitian/project/WaterOS_refactor/os/kernel-rv-final \
  --image /home/zhitian/project/WaterOS_refactor/os/sdcard-rv-pub.img \
  --run-id <exp>-pchot-300 \
  --timeout 300 \
  --plugin pc-hot \
  --output-root /home/zhitian/project/WaterOS_refactor/os/tem/perf/buildstorm
```

分析：

```bash
cd /tmp/wateros-perf-tools.waxdhM/os
scripts/pc-hot/pc-hot-rv.sh analyze \
  /home/zhitian/project/WaterOS_refactor/os/tem/perf/buildstorm/<exp>-pchot-300/pc-hot.txt \
  /home/zhitian/project/WaterOS_refactor/os/kernel-rv-final 80
```

## 下一步建议

1. 决定 `perf/sched-nice-fastpath` 是否继续：要么完成完整验证，要么回退并记录。
2. 优先分析 `with_scheduler` 查询路径的锁和发布开销，而不是继续“少构造快照”。
3. 只对明确能减少 scheduler 全局锁获取/持锁时间的调用链做实验。
4. 可继续看 pc-hot 中的 `set_nice/get_nice`、`VirtQueue::add_notify_wait_pop`、
   `ProcessRegistry` 退出清理、`mount_namespace_snapshot` 的调用方，但每个方向都要先
   写方案并做完整 A/B。

## 交接提示词（可直接发给下一个 agent）

```text
你的任务依旧是 WaterOS BuildStorm 性能优化。当前 main HEAD 是 029dd86b，
main 完整 BuildStorm 中位数约 809.4s，Linux RISC-V baseline 为 395.90s，
阶段一目标约 791.8s。

先阅读：
- AGENTS.md
- os/AGENTS.md 和 os/AGENT.md
- docs/tasks/perf/performance-optimization-handoff-20260811.md
- docs/tasks/perf/2026-08-10-linux-baseline-optimization-roadmap.md
- docs/tasks/perf/heap-optimization-handoff-20260810.md
- docs/tasks/perf/waithot-full-analysis-20260807.md
- docs/tasks/perf/linux-baseline-optimization-log.md
- 旧分支 rejected 文档：
  git show cc14dde6:docs/tasks/perf/2026-08-11-task-lightweight-runtime-stats.md
  git show eec7f3b4:docs/tasks/perf/2026-08-11-current-process-pid-fastpath.md

当前有一个未验证分支 perf/sched-nice-fastpath，未提交。它减少了 setpriority
路径的一次 scheduler 锁和一次完整快照。你可以继续验证，也可以回退；不要默认它能
合并。用户提示过：以前把完整快照改成轻量查询没有完整轮收益，主要成本在全局锁和
BTree 查询，不要重复这个误区。

工作方式：
1. 每个优化先写方案文档，说明为什么选它、方案是什么、为什么这么做、怎么验证。
2. 独立实验放独立 perf/ 分支，有效再合 main，无效回退并记录。
3. 优先优化内核调用路径和锁获取，不要主攻 compiler-builtin、memcpy/memcmp 等基础符号。
4. 完整验证用 os/tem/perf/buildstorm，runner 在
   /tmp/wateros-perf-tools.waxdhM/os/scripts/perf/buildstorm_runner.py；
   如果路径不存在，按交接文档从 perf/tlsf-slab 重建临时 worktree。
5. 每次完整轮用 -snapshot，先双架构 make check，再跑 BuildStorm，需要时补 pc-hot。
6. 不要提交或清理用户未跟踪文件；user 子模块保持原样。
7. 白天是否允许全量测试以我最新指示为准。

先给我一份现状理解和下一步计划，再开始第一个优化。
```
