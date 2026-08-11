# 性能任务：修复就绪队列 `versions` 泄漏（D2）

## 任务目标

修复 `OtherReadyQueue.versions: BTreeMap<TaskId, u64>` **只增不删** 的泄漏：任务 reap/discard 后删除对应条目，避免 fork/exit 大量执行后调度与堆性能退化。

## 背景（必读）

- `docs/todo/perf-fork-exit-degradation.md` §D2
- 两份实现需同步：`impl-multi-class/src/queues.rs`、`impl-round-robin/src/queues.rs`

## 执行前必须参考的 prompt

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`

## 执行前必须参考的文档

- `docs/todo/perf-lock-resource.md`（H-13 相关）
- `docs/exports/features/wateros-task.md`（若存在）

## 需要优先查看的源文件

| 文件 | 用途 |
|------|------|
| `os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/queues.rs` | `bump_version` / `versions` |
| `os/components/wateros-task/task-scheduler/scheduler-impl/impl-round-robin/src/queues.rs` | 同上 |
| `os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/scheduler.rs:544-556` | `reap_exited_task` / `discard_unstarted_task` |
| `os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/registry.rs:501-512` | `reap_task` / `discard_task` |
| `os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/wait_queues.rs:514-527` | reap 路径 |

## 实施要点

1. 为 `OtherReadyQueue` 增加 `forget_task(&mut self, task_id: TaskId)`：`versions.remove(&task_id)`；可选同步清理已无用的 stale `ready_queue` 条目（非必须）。
2. 在 `MultiClassScheduler::reap_exited_task`、`discard_unstarted_task` 中调用；round-robin 调度器对称修改。
3. 确保 `detach_task` / `bump_version` 语义不变：forget 仅在 TCB 已从 registry 移除或确定不再调度时调用。
4. 添加单元测试：`enqueue` → `detach`/`reap` → `versions` 不含该 task_id；或 fork 模拟多次 allocate_id 后 versions.len 有界。

## 验收标准

- [ ] `make rv_check && make la_check` 通过
- [ ] 单元测试覆盖 versions 回收（可放在 `queues.rs` 现有 `#[cfg(test)]` 旁）
- [ ] multi-class 与 round-robin **行为一致**
- [ ] 无新增死锁/双重 detach

## 完成后的回填

- 更新 `docs/todo/perf-fork-exit-degradation.md` 实施状态（可选一行）

## 任务完成自检清单

- [ ] 两处 `queues.rs` 均已修改
- [ ] reap 与 discard 路径均调用 forget
- [ ] 测试证明 versions 不随累计 fork 线性增长

## 示例：交给 Agent 的一次性用户 prompt

```
@docs/tasks/perf/wave1-fix-scheduler-versions-leak/task.md

请修复 OtherReadyQueue.versions 泄漏，reap/discard 时 remove。
同步 multi-class 与 round-robin，加单元测试，make rv_check && la_check。
```
