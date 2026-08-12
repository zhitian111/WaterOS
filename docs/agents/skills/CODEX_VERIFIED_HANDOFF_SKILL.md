---
name: verified-handoff
description: Create, verify, import, or update a high-completeness Codex task handoff when work must move between chats, worktrees, hosts, or agents. Trigger for handoff, 交接, 换对话继续, 总结进度给下一对话, 接管旧任务, or refresh handoff state.
---

# Verified Handoff

Use this skill to preserve a coding task as a verifiable, repository-grounded state package instead of a conversational summary.

## Inputs

Determine or ask only when truly unavailable:

- mode: `EXPORT`, `IMPORT`, or `UPDATE`;
- task ID;
- handoff path;
- whether the task has kernel/QEMU-specific state.

Defaults:

- export directory: `docs/agent/handoffs/<task-id>/`;
- handoff file: `docs/agent/handoffs/<task-id>/HANDOFF.md`;
- template: `assets/HANDOFF_TEMPLATE.md`.

## Mode selection

- `EXPORT`: current chat is handing unfinished work to another chat or environment.
- `IMPORT`: current chat is receiving an existing handoff.
- `UPDATE`: current chat keeps working but must refresh the living handoff.

## EXPORT

1. Read `references/EXPORT_PROMPT.md`.
2. Read `assets/HANDOFF_TEMPLATE.md`.
3. Follow both completely.
4. Inspect the full conversation and current repository; do not rely only on memory.
5. Write the completed handoff and evidence package to the selected output directory.
6. Re-read and audit it before declaring it ready.

## IMPORT

1. Read `references/IMPORT_PROMPT.md`.
2. Read the supplied handoff completely.
3. Perform a read-only discrepancy audit before modifying anything.
4. Preserve all current changes and runtime state.
5. Continue from the first still-valid NEXT item when no blocking discrepancy exists.

## UPDATE

1. Read `references/UPDATE_PROMPT.md`.
2. Preserve the decision, requirement, rejection, and work history.
3. Refresh dynamic repository, validation, runtime, risk, and next-action sections.
4. Re-audit consistency.

## Non-negotiable rules

- Separate user requirements, verified facts, observations, inferences, hypotheses, decisions, and TODOs.
- Record exact Git branch, full HEAD, dirty state, staged/unstaged/untracked/required-ignored files.
- Distinguish user-owned changes from agent-created changes.
- Record changed files and symbols, not only a prose summary.
- Every validation result needs the exact command, CWD, time, exit code, applicable HEAD, dirty state, and log path.
- Record failed and rejected approaches so the receiving agent does not repeat them.
- Preserve non-Git state: processes, QEMU/GDB, ports, mounts, loop devices, containers, temporary data, and reconstruction commands.
- For kernel/QEMU tasks, record architecture, target, toolchain, linker, boot chain, complete QEMU/GDB commands, images/hashes, DTB/devices, trap state, serial logs, and profiling state.
- Never include secrets.
- Never use destructive Git or system operations merely to make actual state match a stale handoff.
- A handoff is not `READY` unless a new chat can determine the goal, done criteria, current implementation, actual evidence, remaining uncertainty, protected state, and exact next action without reading the old chat.
