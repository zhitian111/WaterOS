# K-07B：Fork 页表、COW 与地址空间回收

## 任务目标

在 K-04 证明 fork/destroy 是瓶颈后，先修回收与锁边界，再评估 feature-gated 页表结构
COW。该任务依赖 K-06C 的 retired process 接口，不与其并行修改同一生命周期 API。

## 执行前必读

- `docs/tasks/known-issues/07-mm-exec-fork-heap/task.md`
- `docs/tasks/known-issues/06c-process-reap-lifecycle/task.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-mm.md`
- `docs/todo/perf-memory.md`
- `docs/tasks/perf/wave3-fork-exit-deep-opt/task.md`

## 已知信息与代码证据

当前 fork 会复制页表结构，历史分析认为大 N fork 和 destroy 成本高。结构共享必须同时
实现引用与最后回收，不能只在 fork 增加 ref：

```text
fork share -> page-table write break -> unmap/drop dec_ref -> last owner frees
```

MAP_SHARED frame refs、COW refs 和未来 user pin 不是同一概念。

## 涉及文件

- `os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs`
- `os/components/wateros-mm/mm-impl/impl-loongarch64/src/pagetable.rs`
- `os/components/wateros-mm/mm-api/api-v0/`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/clone.rs`
- `os/components/wateros-task/task-impl/impl-core/src/process.rs`

## 任务内容

1. 记录 fork 页表 frame、walk、copy 与 destroy 时间。
2. 先保证地址空间在 registry 锁外销毁，中间页表和 data frame 全部可回收。
3. 只有仍为 Top 3 才设计结构 COW：ownership、generation、break-COW、rollback 和
   shootdown 必须完整。
4. 两架构共享 API/不变量，各自实现页表格式。
5. 高风险路径默认关闭，可回退现有深拷贝。

## 如何验收

- [ ] fork/COW/MAP_SHARED/munmap/mremap/exec/wait LTP 通过。
- [ ] 8 核 fork-write-exit 无错页、UAF、double free 或 stale TLB。
- [ ] frame/page-table 计数在循环后回到基线。
- [ ] 三轮 fork/exit/lat_ctx 有稳定收益，feature 关闭无回归。

交付 `docs/tasks/history/known-issues/k07b-YYYYMMDD.md`。
