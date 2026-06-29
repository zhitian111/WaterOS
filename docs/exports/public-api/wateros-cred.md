# wateros-cred — 公共 API

事实来源：聚合 `src/lib.rs`（`impl-root` 默认启用）。

## 启用条件

根 `os/Cargo.toml` 可选依赖 `cred`；`pseudo-shell` 与 syscall 路径默认 `cred/impl-root`。

## 类型再导出（`impl-root`）

`Uid`、`Gid`、`TaskId`、`ProcessCredentials`、`Capability`

## 聚合层函数

| 函数 | 说明 |
|------|------|
| `on_user_task_spawned(tid)` | 新用户任务 → ROOT 凭证 |
| `fork_cred(parent, child)` | fork 复制凭证 |
| `share_cred(parent, child)` | 线程 clone 共享 |
| `on_exec(tid)` | execve 后更新（首版 no-op） |
| `drop_task_cred(tid)` | reap 删侧表 |
| `credentials_for(tid)` | 按 tid 读快照；无条目 panic |
| `current_credentials()` | 当前任务快照 |
| `set_uid` / `set_gid` | privileged 全 ID 更新 |
| `set_reuid` / `set_regid` | real/effective；`None` = Linux `-1` |
| `set_resuid` / `set_resgid` | 含 saved；`None` = 保持不变 |
| `set_supplementary_groups(&[Gid])` | 替换 supplementary 列表 |
| `has_cap` / `may_access_inode` / `may_chown` | 权限查询 |

## api-v0 契约（`cred::api`）

| trait / 类型 | 说明 |
|--------------|------|
| `ProcessCredentials` | 八 ID + 最多 32 个 supplementary 组；`ROOT` 常量 |
| `CredentialBackend` | 生命周期与 `current` |
| `CredentialMutation` | per-task `set_resuid` 等 |
| `AccessCheck` | capability 与 inode 访问 |

## 未导出

- `impl-root` 内部 `PerTaskCredRegistry` 与 `registry()` 静态单例。
- 无 `impl-root` 时仅 `api` 模块类型可用，无运行时 hook。
