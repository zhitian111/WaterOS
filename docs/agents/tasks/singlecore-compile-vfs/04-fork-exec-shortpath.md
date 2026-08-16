# Task 04：短进程 fork/exec 低风险优化

## 任务目标

降低 `rustc/cc/ld` 等短进程创建和替换成本。

## 实施方案

1. fork 时跳过未驻留 lazy VMA 的页表复制。
2. exec 时复用解释器/ELF header 读取，避免重复路径查找。
3. 对常见只读 ELF 页复用 page cache。
4. 保持 COW、ASID/TLB 和进程生命周期语义。

## 涉及文件

- `os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs`
- `os/components/wateros-mm/mm-impl/impl-loongarch64/src/pagetable.rs`
- `os/components/wateros-task/src/lifecycle.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/execve.rs`

## CodeGraph 查询

```bash
cd /tmp/wateros-singlecore-compile-vfs
codegraph explore "fork_cow load_program_from_path"
codegraph impact "fork_user_aspace"
codegraph callers "execve_current"
```

## 验收方式

```bash
cd /tmp/wateros-singlecore-compile-vfs/os
HEAP_ALLOCATOR_FEATURE=heap-slab make rv_check
HEAP_ALLOCATOR_FEATURE=heap-slab make la_check
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-rv-final
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-la-final
git diff --check
```

运行时 RV/LA 各 1 轮 BuildStorm，无 SIGSEGV/COW 异常。

## 完成后

新增 `history/04-brief.md`。

