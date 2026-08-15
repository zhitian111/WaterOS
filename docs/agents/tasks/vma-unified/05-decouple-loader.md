# Task 05：将 `Box<dyn DemandPageLoader>` 从 VMA 中解耦

## 任务目标

让 VMA 只表达“虚拟区间 + 权限 + 文件/匿名身份”，不再每个 VMA 持有独立 loader。
split/merge 时不再复制 loader，降低复杂度和出错概率。

## 实施方案

1. 定义统一 backing 身份：

   ```rust
   enum VmaBacking {
       Anonymous,
       File { file_id: ..., offset: usize },
       Device { ... },
   }
   ```

2. `LazyFileVma` 改为保存 `backing`，不再保存 `Box<dyn DemandPageLoader>`；
3. loader 逻辑移到 fault handler，根据 backing 选择读取来源；
4. 共享文件 VMA 与 lazy 文件 VMA 可先保持独立，但统一 backing 枚举。

## 涉及文件

- `os/components/wateros-mm/mm-impl/common/src/vma/**`
- `os/components/wateros-mm/mm-impl/common/src/cache.rs`
- `os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs`
- `os/components/wateros-mm/mm-impl/impl-loongarch64/src/pagetable.rs`
- `os/components/wateros-mm/mm-impl/impl-*/src/kernel_elf.rs`

## CodeGraph 查询

```bash
cd /tmp/wateros-vma-unified
codegraph explore "DemandPageLoader duplicate_box"
codegraph impact "LazyFileVma"
codegraph callers "load_shared_page load_page"
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

- 双架构检查通过；
- VMA split/merge 不再调用 loader duplicate；
- RV/LA 单核完整 BuildStorm 通过。

## 完成后

新增 `history/05-brief.md`。
