# K-07：MM、ELF、Fork/Exit 与内核堆

## 任务目标

在 K-04 证明 page fault、exec、fork/exit 或 allocator 是 Top 3 后，降低 ELF 装载、
页表复制/销毁和堆碎片成本，同时保持 COW、lazy VMA、MAP_SHARED 与跨核 TLB 生命周期
正确。

## 执行前必读

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-mm.md`
- `docs/exports/features/wateros-runtime.md`
- `docs/exports/features/wateros-task.md`
- `docs/exports/features/wateros-syscall.md`
- `docs/todo/perf-memory.md`
- `docs/todo/perf-fork-exit-degradation.md`
- `docs/todo/perf-risk-assessment.md`
- `docs/tasks/perf/wave2-execve-lazy-map/task.md`
- `docs/tasks/perf/wave3-kernel-heap-allocator/task.md`
- `docs/tasks/perf/wave3-fork-exit-deep-opt/task.md`

## 已知信息与代码证据

- Sv39 与 LoongArch64 均已有 `register_lazy_file_vma()` 和
  `handle_lazy_page_fault()`；ELF loader 也有 `elf-lazy-map` 分支。Sv39 默认启用，
  LoongArch feature 组合须由实际构建确认。
- runtime heap 已提供 linked-list 和 TLSF 后端，但默认仍是 linked-list：

```toml
[features]
default = ["impl-linked-list-allocator"]
impl-linked-list-allocator = ["dep:linked_list_allocator"]
impl-tlsf = []
```

- 页表结构 COW、锁外 destroy 和 allocator 切换都是高影响改动；旧 lmbench
  `fork+/bin/sh` 与长期 fork/exit 退化只提供方向，不是当前 main 的充分证据。
- RIO-02 会修改 user-copy fault progress，RIO-03 会修改 fork/dup OFD 共享；本任务
  不得并行定义冲突契约。
- BuildStorm `rustc` 的一次稳定 SIGSEGV 已定位为 `mremap` 搬迁地址撞入未驻留 lazy
  VMA，并完成双架构修复与初赛 `mremap01..06` 回归；完整 final 门禁仍待夜间验证，
  见 [`k07-mremap-vma-relocation-20260802.md`](./history/k07-mremap-vma-relocation-20260802.md)。

## 涉及文件

- `os/components/wateros-mm/mm-api/api-v0/`
- `os/components/wateros-mm/mm-impl/common/`
- `os/components/wateros-mm/mm-impl/impl-sv39/src/{kernel_elf,pagetable,user_heap_mmap}.rs`
- `os/components/wateros-mm/mm-impl/impl-loongarch64/src/{kernel_elf,pagetable,user_heap_mmap}.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/`
- `os/components/wateros-task/task-impl/impl-core/src/process.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/{clone,execve}.rs`
- `docs/todo/perf-{memory,fork-exit-degradation,risk-assessment}.md`

## 可并行任务

- [`K-07A：ELF lazy map`](../07a-elf-lazy-map/task.md)
- [`K-07B：fork/page table lifecycle`](../07b-fork-pagetable-lifecycle/task.md)

K-07A 与 K-07C 可并行。K-07B 必须等待 K-06C 冻结锁外 retired-process 接口，避免
task 与 MM 同时改写地址空间所有权。

## 任务内容

1. **复验 lazy ELF**：确认两个 final feature tree 是否启用；比较 eager/lazy 三轮
   exec、shell 和 BuildStorm 数据。修复只针对权限、BSS、PT_INTERP、mprotect、
   fork 后 fault 等真实失败。
2. **fault/copy**：统计 minor/major fault 和 page walk；user-copy 部分进度与
   read lease 由 RIO-02/RIO-04 定义，本任务只能优化其内部 walk，不得改变 ABI。
3. **fork/destroy**：先做锁外销毁和有界索引，再考虑页表结构 COW。页表 COW 必须
   feature gated，明确中间页表 ownership、refcount、break-COW 和 rollback。
4. **MAP_SHARED/COW**：审计 fork、munmap、mremap、drop 时 frame refcount；不能把
   user-copy pin 与 COW 映射引用混用。
5. **heap**：用 allocator size buckets 和 fragmentation stress 证明 linked-list
   是瓶颈后，再对照启用 TLSF。保留旧后端回退，保持 interrupt guard 和
   `heap_mem_stats()` API。
6. 每项先实现双架构共同契约，再分别实现 Sv39/LoongArch 机制；不能只优化一边却让
   聚合 API 行为分叉。

高风险页表共享至少要表达所有权：

```rust
struct SharedPageTableNode {
    frame: PhysFrame,
    refs: AtomicUsize,
}
```

这只是语义示例，不要求使用该类型。实现前必须证明 destroy 和 write-fault 的最后
引用回收路径，不得只给 fork 增加引用。

## 如何验收

- [ ] `make rv_check && make la_check`，两个 final feature tree 已保存。
- [ ] eager/lazy 或 linked-list/TLSF 的 A/B 使用相同 workload、镜像和三轮口径。
- [ ] execve、mmap/mprotect/munmap、fork/COW、MAP_SHARED 和 signal stack 回归通过。
- [ ] 8 核 fault/fork/exit 压测无 stale TLB、double free、UAF、错页和数据泄漏。
- [ ] fork/exit 后 frame、page-table frame、VMA 和 heap 使用量回到稳定基线。
- [ ] TLSF 若保留，长时间 fragmentation stress 后延迟不随轮次单调恶化。
- [ ] 页表 COW 等高风险改动有默认旧路径、独立提交和关闭 feature 的完整回归。

结果写入 `docs/tasks/history/known-issues/k07-<subtask>-YYYYMMDD.md`。无稳定收益的
策略改动不得进入最终候选。
