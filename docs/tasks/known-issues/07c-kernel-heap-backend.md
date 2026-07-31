# K-07C：内核堆后端 A/B 与碎片压力

## 当前进展

2026-07-31 已定位并修复一项先于 allocator A/B 的 fork/wait 生命周期泄漏：
`epoll-ltp` 的堆增长主要来自未回收的 32 KiB task 内核栈，而不是 allocator 碎片。
根因、修改和隔离 QEMU 结果见
[`results/k07c-20260731.md`](results/k07c-20260731.md)。本任务的双后端 A/B 和完整
workload 门禁仍未完成。

## 任务目标

用现有 linked-list/TLSF 双后端验证长期 fork/exit 退化是否来自 allocator。只有 TLSF
在相同 workload 下稳定改善且无回归时才切换 final feature。

## 执行前必读

- `docs/tasks/known-issues/07-mm-exec-fork-heap.md`
- `docs/prompts/coding.md`
- `docs/exports/features/wateros-runtime.md`
- `docs/tasks/perf/wave3-kernel-heap-allocator.md`
- `docs/todo/perf-fork-exit-degradation.md`

## 已知信息与代码证据

后端和压力入口已存在：

```rust
#[cfg(feature = "impl-linked-list-allocator")]
use backend_linked_list as backend;
#[cfg(feature = "impl-tlsf")]
use backend_tlsf as backend;
```

默认仍是 linked-list。此任务主要是 A/B、修复后端问题和选择 feature，不是再引入第三
个 allocator。

## 涉及文件

- `os/components/wateros-runtime/runtime-heap-allocator/`
- `os/components/wateros-runtime/runtime-heap-allocator/Cargo.toml`
- `os/Cargo.toml`
- `os/components/wateros-base/base-config/`

## 任务内容

1. 两后端运行同一 fragmentation、fork/exit、BuildStorm 和 CAgent workload。
2. 记录 size bucket、alloc failure、used/free、前后半段延迟和最大连续压力。
3. 检查 interrupt guard、多核互斥、alignment、zero-size、realloc 和 OOM 路径。
4. 修复 TLSF 时保持 `heap_mem_stats()` 和链接段契约。
5. 不通过扩大 `KERNEL_HEAP_SIZE` 掩盖泄漏或碎片。

## 如何验收

- [ ] 两后端双架构 check 和基本运行通过。
- [ ] 压力后 used/free 与资源计数稳定，无 allocator metadata 损坏。
- [ ] TLSF 若被选中，后期延迟不明显劣于前期且三轮 BuildStorm 有稳定收益。
- [ ] linked-list feature 仍可回退，CAgent/LTP 无回归。

交付 `docs/tasks/known-issues/results/k07c-YYYYMMDD.md`。
