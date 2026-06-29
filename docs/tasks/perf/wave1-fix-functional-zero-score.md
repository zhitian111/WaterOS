# 性能任务：修复功能性 0 分项（G4 / G5 / G9）

## 任务目标

修复评测中 **score=0**（非「慢」）的功能/微基准项，恢复可计分：

| 项 | 现象 |
|----|------|
| **G4** | libc-bench `b_regex_search` 两用例，4 配置全 0 |
| **G5** | lmbench musl-rv `Pagefaults` 仅该配置 0 |
| **G9** | busybox `kill 10`、`mv test_dir test`、`rmdir test`，4 配置全 0 |

可 **分子任务** 逐项修，但同一 Agent 对话建议一次只攻一项。

## 执行前必须参考的 prompt

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`

## 执行前必须参考的文档

- `docs/todo/perf-baseline-gap-report.md` §G4/G5/G9
- `docs/tasks/analyze_kernel_log.md`（读日志定位）
- `docs/tasks/run_testsuits_qemu.md`（P2 busybox、P3 lmbench）

## 分项指引

### G4 regex_search

- 查 `/home/zhitian/Downloads/score.txt` 与 benchmark 日志中 regex 段：panic、超时、栈溢出
- 可能涉及：用户栈大小、递归深度、`mmap`/`mprotect`、glibc regex 内部 syscall
- 搜索：`os/components/wateros-mm/**`、`os/components/wateros-syscall/**` 与 signal/stack

### G5 musl-rv Pagefaults

- 对比 glibc-rv / musl-rv 同项日志差异
- 重点：`os/components/wateros-mm/mm-impl/impl-sv39/src/user_heap_mmap.rs`（`handle_brk_page_fault`）
- 仅 musl-rv 失败 → 怀疑 ABI、链接布局或某 syscall 在 musl 路径不同

### G9 busybox kill / mv / rmdir

- `kill 10`：信号投递杀不存在进程？应对 errno 语义
- `mv` / `rmdir`：常连锁（mv ENOSYS → rmdir ENOENT）；查 `renameat2`、`unlink`、`rmdir` syscall
- 参考 `docs/tasks/analyze_kernel_log.md` errno 表

## 验收标准

- [ ] 对应测例在 score 平台或本地 QEMU 复现后 **pass 或 score≥1**
- [ ] `make rv_check && make la_check`
- [ ] 未引入其它 busybox/lmbench 回归

## 示例：交给 Agent 的一次性用户 prompt（任选一项）

```
@docs/tasks/perf/wave1-fix-functional-zero-score.md

请只修复 G9 busybox 的 kill/mv/rmdir 三项 0 分。
先读 os/log 或用户提供的 benchmark 日志定位，再改内核，make rv_check。
```

```
@docs/tasks/perf/wave1-fix-functional-zero-score.md

请只修复 G5：musl-rv lmbench Pagefaults score=0。
对比 musl 与 glibc 日志差异后修缺页路径。
```
