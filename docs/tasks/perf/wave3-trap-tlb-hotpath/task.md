# 性能任务：trap / TLB / syscall 热路径（G7）

## 任务目标

降低 lmbench **Simple syscall (~9µs)**、**read/write (~17µs)**、**stat/open** 等延迟，使 score **> 1.0**。

**高风险**：必须 **Feature Flag** 默认旧路径，灰度启用；参见 `docs/todo/perf-risk-assessment.md` 第 3 层。

## 背景（必读）

- `docs/todo/perf-baseline-gap-report.md` §G7
- `docs/todo/perf-hotpath.md`（H-1、H-2、H-5、H-6、M-1、M-2）
- stat/open 部分依赖 dcache（`wave2-fs-read-path.md`），本任务专注 **trap/TLB/拷贝**

## 执行前必须参考的 prompt

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`

## 需要优先查看的源文件

| 文件 | 改动点 |
|------|--------|
| `os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/asm/trap.asm` | TrapContext 多重拷贝 |
| `os/src/trap_handler.rs` | activate satp、signal 返回 |
| `os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/src/paging.rs` | 全局 `sfence.vma` |
| `os/components/wateros-mm/mm-impl/impl-sv39/src/user_access.rs` | 双次 walk |
| `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/signal.rs:267+` | 每次 syscall 查 signal |
| `impl-loongarch64` 对应 trap/paging | LA 对称 |

## 实施要点（分 PR，每项独立 Flag）

1. **H-1**：TCB 为 TrapContext 唯一缓冲，削减 trampoline↔栈↔TCB 拷贝。
2. **H-5**：trap 入口已在 kernel satp 时跳过重复 activate。
3. **M-1/M-2**：ASID generation + 按 VA/ASID selective flush（替代全局 sfence）。
4. **H-2**：`translate` + `perm` 合并为单次 walk。
5. **H-6**：无 pending signal 时跳过 `SIGNAL_REGISTRY` 锁。

## 验收标准

- [ ] 每项改动独立 feature，默认 **关闭**
- [ ] Flag 开启时：`make rv_check && make la_check`；P3 lmbench Simple syscall/read 改善
- [ ] LTP signal/syscall/mmap 全量或大规模抽样无回归
- [ ] 文档记录不变量（TLB、嵌套 fault、COW）

## 禁止

- 不要在一次 PR 合入全部高风险项
- 不要无 Flag 改全局 TLB 行为

## 示例：交给 Agent 的一次性用户 prompt

```
@docs/tasks/perf/wave3-trap-tlb-hotpath/task.md

请只实现 H-6：无 pending signal 时跳过 registry 查锁。
加 feature flag，默认关。make rv_check，跑 signal LTP 子集。
```

```
@docs/tasks/perf/wave3-trap-tlb-hotpath/task.md

请只实现 H-2：user_access 合并 translate+perm 单次 walk。
feature 灰度，LTP + lmbench read 验证。
```
