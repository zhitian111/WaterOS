# K-03B：musl-rv Pagefaults 0 分

## 任务目标

复验仅 musl-rv lmbench Pagefaults 历史 0 分，定位 ELF 布局、VMA、lazy/COW 或结果
解析中的首个差异。该任务可与 K-03A/K-03C 并行。

## 执行前必读

- `docs/tasks/known-issues/03-functional-zero-scores.md`
- `docs/prompts/general.md`
- `docs/prompts/debug_workflow.md`
- `docs/exports/features/wateros-mm.md`
- `docs/tasks/run_testsuits_qemu.md`
- `docs/todo/perf-memory.md`

## 已知信息与代码证据

Sv39 已有 lazy fault 入口：

```rust
self.handle_lazy_page_fault(&mut allocator, fault_addr, access)
```

历史上 glibc-rv 与两个 LA 配置有效，因此必须比较相同 fault 类型和映射，不应按
`/musl/` 路径增加特殊分支。

## 涉及文件

- `os/components/wateros-mm/mm-impl/impl-sv39/src/{pagetable,user_heap_mmap,kernel_elf}.rs`
- `os/src/trap_handler.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/{mmap,task}/`
- `test_case/` 中 lmbench Pagefaults 入口

## 任务内容

1. 用相同参数分别运行 glibc-rv 与 musl-rv，记录 ELF map、fault VA/access、VMA、
   lazy/COW 分支、退出码和耗时。
2. 最小化为 mmap/touch/unmap 或 brk/touch 程序，确认 fault 是否成功返回用户态。
3. 只修复复现路径，保持 file-backed、anonymous、COW 和 permission fault 区分。
4. 与 K-07 的性能优化解耦，本任务只恢复有效功能结果。

## 如何验收

- [ ] musl-rv Pagefaults 产出有效值，glibc-rv 结果无回归。
- [ ] anonymous/file/COW/lazy fault 定向测试通过。
- [ ] 8 核并发 fault 无错页、UAF 或泄漏。
- [ ] `make rv_check && make la_check` 通过。

交付 `docs/tasks/known-issues/results/k03b-YYYYMMDD.md`。
