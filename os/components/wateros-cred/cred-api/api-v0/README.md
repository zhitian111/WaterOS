# Credential API v0 开发手册

[Credential 总览](../../README.md) · [Syscall cred 手册](../../../../components/wateros-syscall/syscall-impl/impl-kernel/src/sys/cred/README.md)

本 crate 定义 UID/GID 快照、生命周期、修改与权限检查契约，不保存 task 状态，也不做用户拷贝或 errno 转换。`TaskId` 只是数值别名，调用方必须使用内核调度实体 ID，不能传 PID/TID 后依赖数值碰巧一致。

## 数据结构

`ProcessCredentials` 保存 real/effective/saved/fs UID/GID，以及最多 32 个 supplementary GID。各 ID 用途不同：real 表示登录身份，effective 通常参与特权判断，saved 支持恢复身份，fs ID 用于文件权限。新增 syscall 时不要把四者统一读写，除非对应 Linux 操作明确要求。

当前 mutation helper 是 privileged 语义：

- `set_uid/set_gid` 同时更新 real/effective/saved/fs。
- `set_reuid/set_regid` 的 `None` 对应用户 ABI `-1`，任一字段变化后 saved/fs 跟随 effective。
- `set_resuid/set_resgid` 独立修改三元组，fs 始终跟随 effective。
- `set_supplementary_groups` 假定上游已保证长度不超过 32；直接调用前必须检查。

API 不负责判断非特权进程能否执行上述变化。增加完整 set*id 规则时，应先定义校验/错误返回接口，不能继续用无返回值 mutation 后在 syscall 层部分猜测。

## Trait 边界

`CredentialBackend` 管 spawn/fork/exec/reap；`CredentialMutation` 管快照变化；`AccessCheck` 是 capability/inode 授权入口。真实 `impl-root::has_cap` 仅对 effective UID 0 返回 true，API 源码中“恒 true”的旧注释不是当前行为；`may_access_inode` 才是尚未落实、当前恒 true 的占位。

新增字段或能力必须定义：root 默认、fork 复制、CLONE_THREAD 共享、exec 保留/重置、最后线程回收、快照暴露以及 VFS 调用点。API 错误属于领域错误，Linux errno 留给 syscall。

## 增加 credential syscall 示例

例如实现 `setfsuid`：

1. 在快照/API 中增加只修改 fs UID 的方法，并定义旧值返回和权限失败的表达。
2. impl-root 在有效 owner 的共享快照上修改，使线程组共享者立即可见。
3. syscall 校验 32 位输入和授权，调用 API，按 ABI 返回旧 fsuid。
4. 文件权限路径确认读取 `fs_uid` 而不是 effective UID。
5. 测试 root、非 root、fork 隔离、clone 共享、exec 与 task reap 后的缺失条目。

