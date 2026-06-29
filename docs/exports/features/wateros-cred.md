# wateros-cred — 已实现功能快照

## 用途

记录 `wateros-cred` 一级组件当前已落地能力。事实来源：`os/components/wateros-cred/**` 与 `docs/guides/cred-module-design.md`。

## 子 crate 与职责

| 子 crate | 职责 | 状态 |
|----------|------|------|
| `wateros-cred`（聚合） | 生命周期 hook、`set*id` 门面、权限查询再导出 | 已实现 |
| `cred-api/api-v0` | `ProcessCredentials`、`CredentialBackend`/`AccessCheck` trait | 已实现 |
| `cred-impl/impl-root` | per-task 侧表（B2）、root 初始 + privileged set*id | 已实现 |

## Feature 矩阵

| Feature | 效果 |
|---------|------|
| `default` | `api-v0` + `impl-root` |
| `api-v0` | 仅契约类型（无运行时侧表） |
| `impl-root` | 启用侧表与 `task` 依赖（`current_credentials` 等） |

## 已实现能力

- **存储**：`TaskId → ProcessCredentials`；线程 clone 共享 owner 引用计数。
- **生命周期**：`on_user_task_spawned`（ROOT）、`fork_cred`、`share_cred`、`on_exec`（no-op）、`drop_task_cred`。
- **ID 更新**：`setuid`/`setgid`/`setreuid`/`setregid`/`setresuid`/`setresgid`/`setgroups`（privileged 语义）。
- **查询**：`credentials_for` / `current_credentials`；syscall 层 `getuid` 族/`getgroups`/`getresuid` 等。
- **权限**：`has_cap`（root 或占位）、`may_access_inode`（恒 true）、`may_chown`（非 root 有限制）。

## 与 syscall / bring-up 接线

- syscall `sys/cred.rs` 读写聚合门面。
- `fork`/`clone`/`execve`/`waitpid` reap 在 syscall 或 call site 调 hook（`task` 不依赖 cred）。

## 缺口与后续

- `on_exec` 未解析 S_ISUID/S_ISGID（`TODO(cred-exec-setuid)`）。
- VFS `fstat` 仍常返回硬编码 uid/gid=0；`may_access_inode` 未用于 open。
- 无真实 capability 位图与 namespace。

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出 |
