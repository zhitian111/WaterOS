# Codex 交接文件增量更新提示词（UPDATE）

用于同一任务继续工作一段时间后刷新 HANDOFF，而不是重新生成并丢失历史。

```text
[MODE]
UPDATE

[PARAMETERS]
HANDOFF_FILE=docs/agent/handoffs/<TASK_ID>/HANDOFF.md
PRESERVE_HISTORY=yes
REFRESH_GIT_STATE=yes
RUN_SAFE_VALIDATION=auto
```

请把现有 HANDOFF 作为“活的任务状态记录”进行增量更新。

要求：

1. 先完整读取 HANDOFF、当前作用域内的 AGENTS 指令和自上次更新时间以来的相关对话。
2. 独立检查当前 CWD、branch、HEAD、status、diff、untracked、ignored、worktree、submodule 和运行现场。
3. 不要删除旧的设计决策、失败尝试、用户纠正和已完成工作；过时内容标记 `[STALE]` 或移动到历史小节。
4. 刷新以下动态部分：
   - YAML 中的 `updated_at`、branch、HEAD、dirty、task/handoff status；
   - 执行摘要；
   - 需求追踪状态；
   - Git 快照；
   - 修改文件与 symbol；
   - 当前实现状态；
   - 事实/观察/假设；
   - 构建和测试矩阵；
   - 运行进程、QEMU/GDB、mount、loop、端口；
   - 风险、blocker；
   - NEXT 队列；
   - 完整性审计。
5. 将本轮操作追加到工作日志，至少记录：
   - 做了什么；
   - 为什么；
   - 改了哪些文件/symbol；
   - 命令和结果；
   - 哪些要求/假设受影响；
   - 下一步。
6. 新测试结果必须记录 branch、HEAD、dirty、时间、命令、exit code 和日志。
7. 新决策使用新的 DEC ID；新失败方案使用新的 REJ ID；不要复用旧 ID 改写历史。
8. 若某假设已证实或否定：
   - 保留原 HYP 条目；
   - 标记 CONFIRMED/REJECTED；
   - 引用新证据；
   - 将结果同步到 verified facts 或 rejected approaches。
9. 重新生成必要的快照/manifest 和 SHA-256；不要复制秘密。
10. 写完后重新读取 HANDOFF，确认 NEXT-001 与当前状态一致，并输出：
    - 文件路径；
    - branch/HEAD/dirty；
    - 本次更新的主要章节；
    - 新 blocker；
    - 当前 NEXT-001。
