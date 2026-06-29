# 性能任务：合入第 1 层低风险 worktree

## 任务目标

将 worktree **`perf-low-risk-8d52acf0`**（基线 `121045b`）中已验收的 **11 项低风险性能优化** 合入主仓库，避免与后续 wave1~3 重复劳动。

## 背景（必读）

- `docs/todo/perf-risk-assessment.md` §「第 1 层实施状态」
- 已实施：M-8、M-17、H-3 跳表、H-16、I-13、I-15、F-14、F-20、F-21、L-14、L-17
- **注意**：合入前 diff main，可能部分已存在（如 `syscall_nr_dispatch.rs`、I-15 BTreeSet）

## 执行前必须参考的 prompt

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`

## 标准流程

1. 定位 worktree 路径或与用户确认 cherry-pick 范围。
2. 逐项对比 main：跳过已合入项，只合缺失 diff。
3. `make rv_check && make la_check`
4. QEMU `user_bringup_busybox` 或文档记载的 `p1_func.log` 同级验证
5. 更新 `perf-risk-assessment.md` 中「尚未合入 main」表述

## 验收标准

- [ ] 11 项状态与 worktree 一致或注明故意省略项
- [ ] 静态检查与 bringup 无 panic
- [ ] 无无关文件混入 commit

## 示例：交给 Agent 的一次性用户 prompt

```
@docs/tasks/perf/wave0-merge-low-risk-worktree.md

请对比 main 与 perf-low-risk-8d52acf0 worktree，合入尚未存在的低风险优化项。
make rv_check && la_check，更新 perf-risk-assessment 合入状态。用户要求时再 commit。
```
