# Task 03：统一 split/merge/protect/remove/mremap 操作

## 任务目标

把两架构中重复的 VMA 修改逻辑收口到 common 注册表，所有 split/merge/protect/remove
都走同一套区间算法；`mremap` 也通过注册表接口完成，不再手工重建列表。

## 实施方案

1. 实现通用区间编辑原语：

   - `split_range(start, end)`
   - `merge_perm(start, end, perm)`
   - `protect_range(start, end, perm)`
   - `remove_range(start, end)`
   - `move_range(old_start, old_end, new_start)`

2. 每项操作在返回前校验有序和无重叠，必要时在 debug 构建触发 assert；
3. `mremap` 调用 `move_range` + `insert`，不再自行 `drain` 重建；
4. 双架构 `user_heap_mmap.rs` 改为只传语义参数。

## 涉及文件

- `os/components/wateros-mm/mm-impl/common/src/vma/**`
- `os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs`
- `os/components/wateros-mm/mm-impl/impl-loongarch64/src/pagetable.rs`
- `os/components/wateros-mm/mm-impl/impl-sv39/src/user_heap_mmap.rs`
- `os/components/wateros-mm/mm-impl/impl-loongarch64/src/user_heap_mmap.rs`

## CodeGraph 查询

```bash
cd /tmp/wateros-vma-unified
codegraph explore "protect_lazy_file_vmas remove_lazy_file_vmas merge_lazy_file_vma_perm"
codegraph impact "protect_lazy_file_vmas"
codegraph explore "mremap_range"
```

## 验收方式

```bash
cd /tmp/wateros-vma-unified/os
make rv_check
make la_check
make kernel-rv-final
make kernel-la-final
git diff --check
```

- 单测覆盖 split/merge/protect/remove/move；
- RV/LA 单核启动与基础 mmap/mremap 冒烟；
- 无新增 panic 或 SIGSEGV。

## 完成后

新增 `history/03-brief.md`。
