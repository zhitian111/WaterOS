# Root Credential 后端手册

[Credential 总览](../../README.md) · [API v0](../../cred-api/api-v0/README.md)

该实现以三张 `BTreeMap` 保存 task credential：`owners[tid] -> owner`、`creds[owner] -> snapshot`、`ref_counts[owner] -> share count`。普通任务 owner 为自身；CLONE_THREAD 指向父 effective owner；fork 建立独立 owner 和快照。

## 生命周期事务

```text
首个用户任务 -> on_user_task_spawned -> ROOT snapshot
fork -> fork_cred(parent, child) -> copy snapshot
clone thread -> share_cred(child,parent) -> owner ref++
clone/fork 失败 -> drop_task_cred(child)
exec -> 当前后端 no-op；syscall 路径另行处理 setuid/setgid 文件位
reap -> drop_task_cred -> owner ref--，最后引用删除 snapshot
```

注意顶层 facade 会拒绝立即删除 current task 的 credential，真正回收留到 reap。这是退出收尾与 zombie 查询所需，不应为“及时释放”而绕过。

所有 map 访问由一个 `MultiprocessorSafeCell` 独占保护，返回 `Copy` 快照。锁内不得用户拷贝、VFS I/O、调度或跨组件 hook。全局 lazy init 当前是 Acquire/Release 检查后写入而非 once/CAS；必须在并发用户任务启动前单线程预热，若重构初始化应使用可证明的一次发布协议。

## 权限现状

- `has_cap`：effective UID 0 拥有当前枚举能力；非 root 无能力集合。
- `may_chown`：root/CAP_CHOWN 放行；非 root 需 fsuid 匹配 inode，只能保持 UID，GID 限于 effective/supplementary group。
- `may_access_inode`：当前恒 true，不能把它当完成的 mode/ACL 权限模型。
- `on_exec`：当前 no-op，set-id 文件处理仍在 exec syscall 接线中。

把能力模型补完整时不要只增加枚举：需要保存 effective/permitted/inheritable/bounding/ambient 集、定义 fork/exec/setuid 变化，并接入 task PCB 中已有 capability 状态或消除重复真相。

## 回归检查

验证同一 owner 的两线程修改可见、任一线程退出不提前删除、最后线程清理、fork 修改隔离、child 初始化失败回滚、TaskId 复用不继承旧身份、组数边界、非 root chown 规则，以及并发首次访问。缺失条目的普通查询会 panic；竞争敏感路径应用 `try_credentials_for`。

