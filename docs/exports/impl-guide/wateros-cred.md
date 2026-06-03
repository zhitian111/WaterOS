# wateros-cred 新增 impl 指南

## 新增 impl 的基本步骤

新增凭证实现时应保持 `api-v0` / `impl-*` / 聚合层分层，避免让 `wateros-task` 反向依赖 cred。

## 需要修改的文件

| 文件 | 要点 |
|------|------|
| `os/components/wateros-cred/Cargo.toml` | 将新的 `cred-impl/impl-*` 加入 workspace members、依赖与 feature |
| `os/components/wateros-cred/src/lib.rs` | 通过 `cfg(feature = "...")` 选择新的 active impl，保持聚合层公开接口稳定 |
| `os/components/wateros-cred/cred-api/api-v0/src/lib.rs` | 仅在契约确实变化时修改 trait/type |
| `os/Cargo.toml` | 平台 feature 需要切换实现时传递新的 `cred/impl-*` feature |
| `os/feature-tree.txt` | feature 链变化后重新导出 |
| `docs/exports/` | 同步 public-api、features、architecture 相关导出 |

## impl 需要提供的能力

新的 impl 至少需要覆盖：

- `CredentialBackend::current`
- `CredentialBackend::on_user_task_spawned`
- `CredentialBackend::fork_cred`
- `CredentialBackend::on_exec`
- `CredentialBackend::drop_task_cred`
- `CredentialMutation::set_resuid`
- `CredentialMutation::set_resgid`
- `AccessCheck::has_cap`
- `AccessCheck::may_access_inode`

聚合层当前还依赖以下门面函数存在：

- `on_user_task_spawned`
- `fork_cred`
- `share_cred`（若实现区分进程与线程凭证所有权）
- `on_exec`
- `drop_task_cred`
- `current_credentials_for`
- `set_resuid`
- `set_resgid`
- `has_cap`
- `may_access_inode`

## 行为边界

`impl-root` 是 bring-up 策略：用户任务初始为 root，set*id 按 privileged 语义放行，capability 与 VFS 权限检查恒 true。真实安全语义、capability 位图、namespace、VFS path permission 与 execve S_ISUID/S_ISGID 均应作为后续 impl/策略演进处理。

## 通用检查清单

- 新 impl crate 是否加入 workspace members。
- impl crate 是否依赖 `wateros-cred-api-v0`，而不是复制 API 类型。
- 聚合层公开函数名是否保持稳定。
- 根 `os/Cargo.toml` 是否传递正确 feature。
- `wateros-syscall` 是否仍只经聚合层调用 cred。
- `wateros-task` 是否仍不依赖 cred。
- 相关导出文档与 `docs/guides/cred-module-design.md` 是否同步。
