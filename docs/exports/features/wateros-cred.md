# wateros-cred 功能快照

## 用途

记录 **`wateros-cred`** 一级组件当前提供的进程凭证能力，便于与 syscall、task 生命周期、VFS 权限占位对照。

## 事实来源

- `docs/guides/cred-module-design.md`
- `os/components/wateros-cred/Cargo.toml`
- `os/components/wateros-cred/src/lib.rs`
- `os/components/wateros-cred/cred-api/api-v0/src/lib.rs`
- `os/components/wateros-cred/cred-impl/impl-root/src/lib.rs`

## 聚合层与 feature

- 根 crate **`wateros`** 在 `qemu-riscv64-opensbi` 与 `qemu-loongarch64-virt` feature 中启用 `cred/api-v0`、`cred/impl-root` 与 `dep:cred`。
- **`api-v0`** 导出 `Uid`、`Gid`、`ProcessCredentials`、`Capability`、`CredentialBackend`、`AccessCheck` 等契约。
- **`impl-root`** 是 bring-up 阶段的默认实现：用户任务凭证初始化为 root，set*id 按 privileged 语义更新当前凭证。

## 当前已具备能力

| 能力 | 状态 | 要点 |
|------|------|------|
| per-task 凭证侧表 | 已接入 | `TaskId -> ProcessCredentials`；无条目读取会 panic 且消息含 `tid` |
| 用户任务 spawn | 已接入 | `on_user_task_spawned(tid)` 写入 `ProcessCredentials::ROOT` |
| fork 凭证继承 | 已接入 | `fork_cred(parent, child)` 复制父凭证 |
| thread clone 凭证共享 | 已接入 | `share_cred(parent, child)` 通过 owner/refcount 共享凭证槽 |
| execve 钩子 | 占位 | `on_exec(tid)` no-op，保留 `TODO(cred-exec-setuid)` |
| reap 清理 | 已接入 | `drop_task_cred(tid)` 删除侧表条目；不存在时 no-op |
| 当前任务凭证 | 已接入 | `current_credentials()` 读取当前 task id 后查询 impl |
| set*id 更新 | 已接入 | impl-root 按 privileged 语义更新 real/effective/saved/fs id；re/res 的 `-1` 表示保持不变 |
| capability / inode 权限 | 占位 | `has_cap` / `may_access_inode` 在 impl-root 恒返回 true |

## Syscall 对接

- `getuid` / `geteuid` / `getgid` / `getegid` 读取当前 `ProcessCredentials` 并返回对应 ID。
- `getgroups` 当前返回固定 supplementary group `[0]`，`getgroups(0, NULL)` 返回数量 `1`。
- `setuid` / `setgid` / `setreuid` / `setregid` / `setresuid` / `setresgid` 更新当前任务凭证，成功返回 `0`；非法超宽 uid/gid 返回 `EINVAL`。

## 明确未覆盖

- 非 root 权限拒绝、capability 位图和 namespace 下的完整 set*id 安全语义。
- capabilities 位图、namespace、`prctl(PR_CAPBSET_*)` 迁移。
- VFS 路径权限检查与 ext4 inode owner 对接。
- execve 的 S_ISUID / S_ISGID 规则。

## 维护要求

修改 `ProcessCredentials` 字段、生命周期 hook、syscall identity 行为或 VFS 权限策略时，同步更新本文件与 `docs/guides/cred-module-design.md`。
