# 任务 16：munmap 按实际移除 PTE 同步并删除重复 fence

## 任务内容与目标

让 munmap 在只删除未驻留 lazy VMA metadata 时不做 TLB 同步；仅在确实移除 resident PTE
时刷新。消除实现内部 `fence_user_ptes()` 与 syscall facade 外层 flush 的重复，并为实际范围
同步保留升级空间。

## 实施方案

1. `unmap_mmap_range` 统计是否移除叶 PTE，并返回 `PteChange`；未映射页不记变化。
2. `MmapOps::munmap`、`munmap_external` 和相关 remove helper 传播摘要。
3. 同步责任只保留在 user-aspace facade；错误路径继续 full flush。
4. 若远端 shootdown 协议仍只支持 ASID 全量，先实现 None/Full，不伪造 Range；Range 需同时
   扩展 IPI payload、ack 和两架构 handler。
5. 测试 lazy-only、部分 resident、全部 resident、共享/设备页、重复 munmap 和并发 CPU。

## 涉及文件

- `os/components/wateros-mm/mm-api/api-v0/src/mmap.rs`
- 双架构 `pagetable.rs`、`user_heap_mmap.rs`、`user_aspace.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/mem/mmap.rs`

## CodeGraph 查询

```bash
codegraph explore "MmapOps::munmap unmap_mmap_range fence_user_ptes request_tlb_shootdown"
codegraph impact "unmap_mmap_range"
codegraph callers "MmapOps::munmap"
```

## 验收方式

```bash
cd os
make rv_check && make la_check
make kernel-rv-final && make kernel-la-final
# munmap/mremap/shared/device 与 SMP stale-TLB 定向回归
cd .. && git diff --check
```

lazy-only munmap 的 flush/shootdown 计数为零；resident PTE 仍恰好同步一次。任务 00 runner A/B
记录 skip 比例、shootdown 数和完整 BuildStorm 结果。

## Commit 与简报

提交建议：`[perf] munmap 仅同步实际移除的 PTE`。新增 `history/16-brief.md`。
