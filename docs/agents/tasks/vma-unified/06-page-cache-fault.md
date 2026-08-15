# Task 06：文件缺页统一走 page cache/backing 路径

## 任务目标

把只读文件页、私有文件页、匿名零页的缺页逻辑统一到一条更接近 Linux 的路径：
VMA 负责区间与权限，backing/page cache 负责内容。

## 实施方案

1. 文件 backing 缺页：

   - 只读页优先 `mm-impl/common/cache.rs` 的只读页缓存；
   - 未命中时读文件并发布缓存帧；
   - 私有写页分配新帧，内容来自 backing。

2. 匿名 backing 缺页：

   - 分配零帧并映射；
   - 写时 COW 继续走现有 frame refcount。

3. 两架构 page fault handler 调用统一入口，减少架构内重复逻辑。

## 涉及文件

- `os/components/wateros-mm/mm-impl/common/src/vma/**`
- `os/components/wateros-mm/mm-impl/common/src/cache.rs`
- `os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs`
- `os/components/wateros-mm/mm-impl/impl-loongarch64/src/pagetable.rs`
- `os/components/wateros-vfs/vfs-impl/impl-page-cache/src/**`

## CodeGraph 查询

```bash
cd /tmp/wateros-vma-unified
codegraph explore "load_or_get_readonly_elf_page handle_lazy_page_fault"
codegraph impact "load_or_get_readonly_elf_page"
codegraph explore "file_cache"
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

运行时：

- RV/LA 单核、RV 8 核、LA 12 核完整 BuildStorm；
- 无 SIGSEGV/panic/OOM；
- 只读文件页共享统计不出现负引用/双释放。

## 完成后

新增 `history/06-brief.md`。
