# Task 01：移除默认热路径的全局 slab 统计原子操作

## 任务目标

修复当前最明确的退化源：`SLAB_ALLOC_COUNT` 和 `SLAB_DEALLOC_COUNT` 在每个 alloc/free
上对共享缓存线执行 `fetch_add`。默认 Final 必须不执行该操作；诊断统计只能通过独立
feature 或 per-CPU 普通计数实现。

## 实施方案

1. 将 `heap_slab_stats()` 和计数存储置于明确的诊断 feature 下，生产 `impl-slab` 只保留
   allocator 路径。
2. 若保留统计接口，使用 per-CPU 槽位，在 allocator guard 已关闭本地中断的前提下采用
   owner-only 普通整数；汇总时再读取，不引入全局 RMW。
3. 保持 API 关闭 feature 时的条件导出一致；不要在默认构建中保留未使用的计数符号。

## 验收方式

```bash
cd os
make rv_check
make la_check
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-rv-final
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-la-final
git diff --check
```

生产内核反汇编/符号检查：

```bash
llvm-nm -n kernel-rv-final | rg 'SLAB_(ALLOC|DEALLOC)_COUNT' && exit 1 || true
llvm-nm -n kernel-la-final | rg 'SLAB_(ALLOC|DEALLOC)_COUNT' && exit 1 || true
```

功能：运行 slab 自检、短 BuildStorm smoke，检查无递归分配、OOM、SIGSEGV、错误脚本结果。
性能仅运行 RISC-V：在同一宿主用任务 00 的 main/TLSF 与候选 slab 做 A/B/B/A，各至少一轮；单轮不能
出现超过 2% 的稳定回退，若回退则不得进入任务 02。

## 涉及文件与 CodeGraph

- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/mod.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/lib.rs`
- 可能新增 `slab/diagnostics.rs`

```bash
codegraph explore "SLAB_ALLOC_COUNT SLAB_DEALLOC_COUNT heap_slab_stats alloc_on dealloc_on"
codegraph callers "heap_slab_stats"
```

## 完成后

新增 `history/01-brief.md`，列出默认内核是否仍含计数符号、双架构检查、RISC-V A/B 结果和未验证项。
