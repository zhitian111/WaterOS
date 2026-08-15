# Task 02：建立有序、无重叠的 VMA 注册表/索引

## 任务目标

把 `lazy_file_vmas` 从“手工维护的 `Vec` + 二分查找”改为统一注册表，保证每次修改后
仍满足：

- 按 `start` 有序；
- 任意两 VMA 不重叠；
- 查找不再依赖调用方记住“列表有序”的隐含约定。

## 实施方案

1. 在 common VMA 模块定义 `VmaRegistry`：

   - 内部可用 `alloc::collections::BTreeMap` 或有序 `Vec`；
   - 提供 `lookup(page)`、`insert(vma)`、`remove_range(start,end)`、`protect_range()`；
   - 每次修改后执行排序/去重/重叠校验。

2. 两架构 `Sv39AddressSpace` / `LoongArch64AddressSpace` 持有该注册表，不再直接暴露
   可变 `Vec` 给各调用方。

3. 删除或收窄以下公开字段：

   ```rust
   pub(crate) lazy_file_vmas: Vec<LazyFileVma>
   ```

4. 先保持外部行为不变，仅替换内部实现。

## 涉及文件

- `os/components/wateros-mm/mm-impl/common/src/vma/**`
- `os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs`
- `os/components/wateros-mm/mm-impl/impl-loongarch64/src/pagetable.rs`
- `os/components/wateros-mm/mm-impl/impl-*/src/user_heap_mmap.rs`

## CodeGraph 查询

```bash
cd /tmp/wateros-vma-unified
codegraph impact "lazy_file_vmas"
codegraph callers "register_lazy_file_vma"
codegraph explore "lazy_file_vma_index lazy_vma_overlaps"
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

- `rg -n "lazy_file_vmas" components/wateros-mm` 中除注册表实现外不应再有直接修改；
- 单测覆盖插入、查找、split、merge、remove、protect 后的有序/无重叠不变量；
- RV/LA 单核冒烟通过。

## 完成后

新增 `history/02-brief.md`。
