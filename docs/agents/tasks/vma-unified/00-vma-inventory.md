# Task 00：盘点双架构 VMA 结构、调用路径与有序性不变量

## 任务目标

产出 VMA 现状清单，明确 Sv39 与 LoongArch64 重复实现、修改路径、隐含不变量，
以及当前 lazy VMA 二分查找失序风险点。本任务不修改代码，只提交盘点文档。

## 实施方案

1. 用 CodeGraph 查询 VMA 符号和调用链；
2. 对照两套 `pagetable.rs` 和 `user_heap_mmap.rs`；
3. 列出：
   - 四类 VMA：`LazyFileVma`、`SharedAnonVma`、`SharedFileVma`、`DeviceVma`；
   - 修改函数：register/remove/merge/protect/mremap；
   - 依赖的有序性假设；
   - 可能的失序入口。
4. 输出到 `history/00-brief.md` 或本目录盘点文档。

## 涉及文件

- `os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs`
- `os/components/wateros-mm/mm-impl/impl-sv39/src/user_heap_mmap.rs`
- `os/components/wateros-mm/mm-impl/impl-loongarch64/src/pagetable.rs`
- `os/components/wateros-mm/mm-impl/impl-loongarch64/src/user_heap_mmap.rs`
- `os/components/wateros-mm/mm-impl/common/src/lib.rs`

## CodeGraph 查询

```bash
cd /tmp/wateros-vma-unified
codegraph explore "LazyFileVma register_lazy_file_vma protect_lazy_file_vmas"
codegraph impact "LazyFileVma"
codegraph callers "handle_lazy_page_fault"
codegraph explore "mremap_range lazy_file_vmas"
```

## 验收方式

- 完成盘点文档；
- `git diff --check` 通过；
- 文档中明确标出两架构重复代码和可疑失序路径；
- 不改变任何内核代码。

## 完成后

新增 `history/00-brief.md`。
