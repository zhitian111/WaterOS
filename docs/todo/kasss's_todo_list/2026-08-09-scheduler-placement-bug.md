# 调度器入队策略导致 running task 被迁移到其他核

**日期**: 2026-08-09  
**分支**: sche  
**状态**: 已修复

## 症状

```
[PANIC] [sched] publishing running task 44 to CPU 2 while it runs on CPU 6
  left: CpuId(6)
 right: CpuId(2)
```

`runqueue.rs:213` 断言失败：`enqueue_ready_on_cpu` 检测到任务 44 仍在 CPU 6 执行，但被尝试入队到 CPU 2 的就绪队列。

## 根因

将 `ReadyPlacement::Prefer`（yield/tick 重入队专用）改为 `LastCpu` + 过载检测后：

```
CPU 6: schedule_tick → enqueue(task44, LastCpu)
       → cpu_is_overloaded(6) = true（有空闲核 CPU 4）
       → fallback 到 ring scan → 选 CPU 2
       → task44 入队到 CPU 2
       → 但 CPU 6 还没执行 __switch！
       → task44 仍在 CPU 6 上 running → 断言失败
```

关键错误：yield/tick 路径的当前任务**还未切出**（`__switch` 未执行），不能通过过载检测迁移到其他核。`deferred_ready` 机制就是为此设计的——标记待迁移，在 `__switch` 后才发布。

## 修复

恢复 `Prefer(CpuId)` 专用于 yield/tick 重入队（始终留在当前核）。最终的入队策略：

| 场景 | 放置策略 | 过载检测 | 原因 |
|------|---------|---------|------|
| Yield/Tick 重入队 | `Prefer(当前核)` | ❌ | 任务还在运行，不能迁移 |
| Wakeup | `LastCpu` | ✅ 过载→fallback | 任务已切出，安全迁移 |
| New/Deferred | `LeastLoaded` | — | 选最空闲核 |

`cpu_is_overloaded` 的空闲核感知保留，仅在 wakeup/`LastCpu` 路径生效。

## 经验教训

- `Prefer` 和 `LastCpu` 不能简单互换。`Prefer` 保证不迁移正在执行的任务，`LastCpu` 允许过载时溢出。
- 任何试图把"当前正在某核上运行"的任务放到其他核的入队操作都是错误的——必须走 deferred 路径（标记 → `__switch` → `enqueue_deferred`）。
