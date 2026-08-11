# K-07A：双架构 ELF lazy map 复验

## 任务目标

确认 RV/LA final feature 实际启用 ELF lazy map，修复当前仍存在的权限、BSS、
PT_INTERP 或 fault 问题，并量化 exec/shell 收益。该任务可与 K-07C 并行。

## 执行前必读

- `docs/tasks/known-issues/07-mm-exec-fork-heap/task.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-mm.md`
- `docs/tasks/perf/wave2-execve-lazy-map/task.md`

## 已知信息与代码证据

两架构 loader 都已有 feature 分支：

```rust
#[cfg(feature = "elf-lazy-map")]
return map_segment_from_path_lazy(/* ... */);
```

Sv39 crate 默认启用，LoongArch 是否启用必须以 `feature-tree.txt` 为准。

## 涉及文件

- `os/components/wateros-mm/mm-impl/impl-{sv39,loongarch64}/src/kernel_elf.rs`
- `os/components/wateros-mm/mm-impl/impl-{sv39,loongarch64}/src/pagetable.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/execve.rs`
- `os/components/wateros-vfs/vfs-api/api-v0/src/mmap.rs`

## 任务内容

1. 保存两个 final feature tree，确认 lazy/eager 分支。
2. 用相同 ELF 比较 exec wall time、初始 mapped page、fault 和 I/O。
3. 验证 file bytes、BSS zero、segment overlap、W^X/mprotect、interpreter 与 auxv。
4. 验证 fork 后 lazy fault、unlink 后已打开 executable 和并发 exec。
5. 只修复真实失败；不与页表结构 COW 同提交。

## 如何验收

- [ ] 两架构 exec/busybox/shell/LTP execve 通过。
- [ ] eager/lazy 三轮对比有稳定收益或明确“不保留”结论。
- [ ] BSS、权限、COW、mmap 和 signal stack 无回归。
- [ ] BuildStorm 和双架构 check 通过。

交付 `docs/tasks/history/known-issues/k07a-YYYYMMDD.md`。
