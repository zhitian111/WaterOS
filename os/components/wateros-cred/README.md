# wateros-cred

[项目首页](../../../README.md) · [内核工程](../../README.md) · [系统架构](../../../README.md#系统架构)

`wateros-cred` 是 WaterOS 的进程凭证聚合模块。它以 `TaskId` 为 key 维护 per-task 凭证侧表，
对齐 Linux `struct cred` 子集（八 ID + supplementary 组），并提供初始 root + privileged
`set*id` 策略。生命周期 hook 由 syscall 路径与 bring-up call site 显式调用；`task` crate
不依赖本组件。

## 模块分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合门面 | `src/lib.rs` | 导出版本化 `api` 与 `impl-root`，提供顶层凭证生命周期与查询/修改入口。 |
| 凭证 API | `cred-api/api-v0/` | Linux `struct cred` 子集：`Uid` / `Gid` / `ProcessCredentials`、`Capability`、`AccessCheck` / `CredentialBackend` / `CredentialMutation` trait。 |
| root 实现 | `cred-impl/impl-root/` | 初始 root + privileged `set*id` 策略，`PerTaskCredRegistry` 全局侧表。 |

## 实现说明

- 生命周期 hook（`on_user_task_spawned` / `fork_cred` / `share_cred` / `on_exec` /
  `drop_task_cred`）由 syscall 路径与 bring-up 显式调用；`wateros-task` 不依赖本组件，避免
  循环依赖。
- impl-root 阶段初始凭证为 `ProcessCredentials::ROOT`（全部 ID 为 0，supplementary 组
  `[0]`），并按 privileged 语义放行 `set*id` 更新（`setuid` / `setgid` / `setreuid` /
  `setregid` / `setresuid` / `setresgid` / `setgroups`）。
- per-task 侧表以 `TaskId` 为 key；`owners` 与 `ref_counts` 支持 clone 线程共享同一份凭证，
  `share_cred` 只增加引用，`drop_task_cred` 在引用归零时删除条目。
- 无侧表条目时查询接口 panic（与 bring-up root 模型一致）；`try_credentials_for` 对“尚未发布
  或已回收”的任务返回 `None`。
- capabilities 与 VFS 权限检查在此 trait 层预留；`prctl(PR_CAPBSET_*)` 的权威语义最终归口
  `AccessCheck::has_cap`。
- `exec` 后的 `on_exec` 首版为 no-op，保留 `TODO(cred-exec-setuid)` 扩展点。

## 调用链路

生命周期（由 syscall / bring-up 显式调用）：

```text
create_user_task(tid)
  -> on_user_task_spawned(tid)        注册 root 凭证侧表条目

fork(parent, child)
  -> fork_cred(parent, child)         复制父任务凭证

clone(parent, child)
  -> share_cred(parent, child)        共享凭证，ref_counts +1

exec(tid)
  -> on_exec(tid)                     首版 no-op（扩展点）

reap / exit(tid)
  -> drop_task_cred(tid)              引用归零后删除侧表条目
```

查询与修改（syscall 层调用）：

```text
getuid / getgid / getgroups 等
  -> credentials_for(tid) / current_credentials()

setuid / setgid / setreuid / setregid / setresuid / setresgid / setgroups
  -> set_uid / set_gid / set_reuid / set_regid / set_resuid / set_resgid ...
```

## 各实现功能

### cred-api / 凭证 API

`cred-api/api-v0/src/lib.rs`：

- `Uid` / `Gid`：Linux 语义下的 32 位用户 / 组 ID。
- `ProcessCredentials`：real/effective/saved/fs uid + gid（八 ID）与固定长度
  `supplementary_groups`（`SUPPLEMENTARY_GROUP_COUNT = 32`）。
  - `ROOT`：bring-up 默认凭证。
  - `set_uid` / `set_gid` / `set_reuid` / `set_regid` / `set_resuid` / `set_resgid` /
    `set_supplementary_groups`：privileged `set*id(2)` 语义。
- `Capability`、`AccessCheck`、`CredentialBackend`、`CredentialMutation`：权限检查与后端 trait
  预留。

### impl-root / root 实现

`cred-impl/impl-root/src/lib.rs`：

- 全局变量：
  - `static mut CRED_REGISTRY: MaybeUninit<MultiprocessorSafeCell<PerTaskCredRegistry>>`：
    per-task 凭证侧表，多核安全 cell。
  - `static CRED_REGISTRY_READY: AtomicUsize`：侧表就绪标志。
- `PerTaskCredRegistry`：
  - `creds: BTreeMap<TaskId, ProcessCredentials>`：凭证主表。
  - `owners: BTreeMap<TaskId, TaskId>`：凭证归属（clone 共享时指向父任务）。
  - `ref_counts: BTreeMap<TaskId, usize>`：共享引用计数。
- 生命周期：`on_user_task_spawned` / `fork_cred` / `share_cred` / `on_exec` / `drop_task_cred`。
- 查询：`current_credentials_for` / `try_credentials_for`。
- 修改：`set_resuid` / `set_resgid`（顶层 `set_uid` / `set_gid` / `set_reuid` / `set_regid`
  委托至此）。

### 聚合门面 / src/lib.rs

- `pub mod api`：重导出 `api_v0`。
- `active_impl`：当前 feature 选中的凭证后端（impl-root）。
- 顶层入口：`on_user_task_spawned` / `fork_cred` / `share_cred` / `on_exec` /
  `drop_task_cred` / `credentials_for` / `try_credentials_for` / `current_credentials` /
  `set_uid` / `set_gid` / `set_reuid` / `set_regid` / `set_resuid` / `set_resgid`。
