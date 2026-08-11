# K-06C：Process reap 与资源锁外销毁

## 任务目标

把 process/task 从 registry 脱离与地址空间、fd、signal/futex 等大资源销毁分为两阶段，
避免持全局锁执行递归 drop，并保证 zombie/wait/parent 生命周期正确。

## 执行前必读

- `docs/tasks/known-issues/06-task-scheduler-futex/task.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-task.md`
- `docs/audits/lock-inventory.md`
- `docs/audits/resource-inventory.md`
- `docs/todo/perf-lock-resource.md`

## 已知信息与代码证据

当前 `reap_process_with_tasks()` 在 remove 闭包中调用 MM drop：

```rust
if let Some(aspace) = process.address_space {
    drop_user_aspace_on_task_exit(aspace.user_aspace_ptr());
}
```

如果外层持 registry 锁，页表销毁会扩大临界区并形成反向依赖风险。

## 涉及文件

- `os/components/wateros-task/task-impl/impl-core/src/process.rs`
- `os/components/wateros-task/src/process.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/`
- `os/components/wateros-mm/mm-api/api-v0/`
- `os/components/wateros-vfs/vfs-impl/impl-fd-session/`

## 任务内容

1. registry 锁内只校验状态、reparent、移除并返回 owned retired object。
2. 锁外按明确顺序清理 task、MM、fd、signal、futex 和 namespace。
3. wait/reap 只能成功一次；最后线程退出前不能提前销毁共享地址空间。
4. 失败/取消路径不得把部分清理对象重新暴露给 registry。
5. 更新锁序和资源生命周期审计。

## 如何验收

- [ ] wait4/waitid/zombie/orphan/exit_group/pthread LTP 通过。
- [ ] 10,000 次 fork/exit 后 registry、frame、fd、futex 数有界。
- [ ] MM/VFS drop 明确在 registry/scheduler 锁外。
- [ ] 无 double reap、UAF、父进程漏唤醒；双架构 check 通过。

交付 `docs/tasks/history/known-issues/k06c-YYYYMMDD.md`。
