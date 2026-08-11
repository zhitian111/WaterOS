# 性能任务：fork/exit 深层优化（M-3 / L-1 / D3）

## 任务目标

降低 **fork+exit**、**ctx switch 大 N setup**、**reap** 成本；配合 `wave3-kernel-heap-allocator.md` 与 `wave1-fix-scheduler-versions-leak.md` 解决进程路径性能。

**高风险项须 Feature Flag**；可与 wave3 其它任务并行但 **不要同 PR 硬塞**。

## 背景（必读）

- `docs/todo/perf-fork-exit-degradation.md` §D3
- `docs/todo/perf-memory.md`（M-3、M-4、M-9）
- `docs/todo/perf-lock-resource.md`（L-1、L-3、L-15）

## 需要优先查看的源文件

| 文件 | 改动点 |
|------|--------|
| `os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs:624-896` | fork 整树复制、destroy_table |
| `os/components/wateros-task/task-impl/impl-core/src/process.rs:112-138,779-803` | alloc_pid O(P×T)、reap 锁内 destroy |
| `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/clone.rs:239-336` | 关中断窗口、fd 复制 |
| `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task.rs:854+` | exit 资源清理 |

## 分项实施

### L-1（中高）：reap 释锁后 destroy

- `reap_process_with_tasks` 仅 remove PCB，**释锁后** `drop_user_aspace`
- 注意 RefCell/关中断与锁序（对照 `docs/audits/locks/*`）

### D3（低中）：PID/TID O(1)

- 位图或空闲栈分配，去掉 `alloc_pid` 内 `task_id_for_thread` 全表扫描

### M-3（高）：页表结构 COW

- fork 共享中间节点 + 写 fault 分裂；**必须 Flag + 大量 fork/COW LTP**

### L-3（中高）：fork fd 释锁 duplicate

- 缩小 `CloneSetupGuard` 关中断范围

## 验收标准

- [ ] 每项独立 PR + feature（M-3 必须）
- [ ] `make rv_check && make la_check`
- [ ] lmbench Process fork+exit、ctx 大 N 改善或稳定
- [ ] wait/exit/zombie LTP 无回归

## 示例：交给 Agent 的一次性用户 prompt

```
@docs/tasks/perf/wave3-fork-exit-deep-opt/task.md

请只做 L-1：reap 释锁后再 destroy 地址空间。
最小 diff，对照 lock-inventory 锁序，make rv_check && la_check。
```

```
@docs/tasks/perf/wave3-fork-exit-deep-opt/task.md

请只做 D3：ProcessRegistry PID/TID 位图 O(1) 分配。
```
