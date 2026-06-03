# wateros-cred 公共 API 快照

## 用途

描述一级组件 **`wateros-cred`** 当前通过聚合 crate 暴露给内核主线的进程凭证接口，以及 `cred-api/api-v0` 中的稳定契约。

## 事实来源

- `os/components/wateros-cred/Cargo.toml`
- `os/components/wateros-cred/src/lib.rs`
- `os/components/wateros-cred/cred-api/api-v0/src/lib.rs`
- `os/components/wateros-cred/cred-impl/impl-root/src/lib.rs`
- `docs/guides/cred-module-design.md`

## 契约层

`cred-api/api-v0` 提供以下公开契约：

| 项 | 说明 |
|----|------|
| `TaskId` | cred 侧表索引，数值与 `task::TaskId` 对齐，但 API 层不依赖 task crate |
| `Uid` / `Gid` | Linux 语义下 32-bit 用户 ID / 组 ID newtype |
| `ProcessCredentials` | real/effective/saved/fs uid/gid 与 supplementary groups 快照 |
| `ProcessCredentials::ROOT` | bring-up 默认 root 凭证，所有 ID 为 0 |
| `SUPPLEMENTARY_GROUP_COUNT` | G1 语义固定为 1 |
| `CredentialBackend` | spawn、fork、exec、reap、current 生命周期契约 |
| `CredentialMutation` | set*id 族更新 real/effective/saved uid/gid 的契约 |
| `Capability` | capability 占位枚举 |
| `AccessCheck` | `has_cap` 与 `may_access_inode` 权限检查占位契约 |

## 聚合层接口

`wateros-cred/src/lib.rs` 在 `impl-root` feature 下导出：

| 接口 | 说明 |
|------|------|
| `api` | 重导出 `cred-api/api-v0` |
| `active_impl` | 当前激活实现，默认 `impl-root` |
| `on_user_task_spawned(tid)` | 新用户任务初始化 root 凭证 |
| `fork_cred(parent, child)` | fork 后复制父任务凭证 |
| `share_cred(parent, child)` | thread clone 后共享父任务凭证槽 |
| `on_exec(tid)` | execve 凭证钩子，首版 no-op |
| `drop_task_cred(tid)` | 任务 reap 后释放侧表条目 |
| `current_credentials()` | 读取当前任务凭证；无当前任务或无条目时 panic |
| `set_uid(uid)` / `set_gid(gid)` | privileged setuid/setgid 语义，更新对应 ID 组 |
| `set_reuid(...)` / `set_regid(...)` | `None` 表示 Linux `-1` 保持不变 |
| `set_resuid(...)` / `set_resgid(...)` | 更新 real/effective/saved ID，`None` 表示保持不变 |
| `has_cap(...)` / `may_access_inode(...)` | P1 权限占位，impl-root 当前恒 true |

## Feature

| Feature | 说明 |
|---------|------|
| `default` | `api-v0` + `impl-root` |
| `api-v0` | 启用 `wateros-cred-api-v0` |
| `impl-root` | 启用 bring-up 阶段 root 凭证实现 |

根 crate **`wateros`** 在 `qemu-riscv64-opensbi` 与 `qemu-loongarch64-virt` feature 中启用 `cred/api-v0`、`cred/impl-root` 与 `dep:cred`。

## 维护要求

修改 `ProcessCredentials` 字段、生命周期 hook、set*id 行为或权限检查策略时，同步更新本文件、`docs/exports/features/wateros-cred.md`、`docs/guides/cred-module-design.md` 与 `docs/architecture/snapshot.md`。
