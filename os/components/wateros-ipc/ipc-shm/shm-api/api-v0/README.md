# SysV SHM API v0 离线开发手册

本文描述共享内存的稳定类型和所有权规则。模块总览见 [ipc-shm](../../readme.md)，实现见
[impl-frame](../../shm-impl/impl-frame/README.md)，syscall 入口见
[sys/ipc](../../../../wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/README.md)。

## 1. 模块边界

API crate 只定义 SysV 标志、错误、ID 和 MM 交接快照。registry 实现拥有段与物理帧；syscall
层负责 ABI、权限、VA 选择与提交/回滚；MM 层负责 PTE、地址空间并发和 TLB shootdown。

关键所有权规则：`ShmSegmentInfo.pages` 与 `ShmAttachInfo.pages` 都是页号快照，复制 `Vec`
不转移物理帧所有权。只有 SHM registry 在段真正销毁时释放帧，MM 解除共享映射时不能把它们
当普通匿名页释放。

## 2. 常量和错误

- `IPC_PRIVATE=0`：每次创建新段，不进入普通 key 索引；
- `IPC_CREAT=0o1000`、`IPC_EXCL=0o2000`；
- `SHM_RDONLY=0o10000`；
- `MAX_SHM_SEGMENT_SIZE=4 MiB`：当前 bring-up 限制，并非完整 Linux 上限。

`ShmError` 到 errno 的通常映射是 `Invalid->EINVAL`、`Exists->EEXIST`、
`NoEntry->ENOENT`、`NoMem->ENOMEM`、`NoSys->ENOSYS`。权限错误由 syscall 层检查并返回
`EACCES/EPERM`，所以 API 错误中没有 permission 变体。

`TaskId` 是 WaterOS 内部任务号，只用于 attachment 表 key，不等于用户 PID/TID。

## 3. 三个公开快照

`ShmSegmentInfo` 包含 shmid/key、页对齐 size、mode、owner/creator、`nattch`、删除标记、
Linux 时间/进程元数据和物理页列表。它服务 `IPC_STAT` 与映射准备，不允许调用者释放 pages。

`ShmAttachInfo` 描述某一次任务映射：`shmid/base/size/readonly/pages`。它足以让 MM 在
registry 锁外建立或撤销共享 PTE，但不是 attachment 的所有权 token。

`ShmRegistryStats` 为 `SHM_INFO`/观测提供段数、总页数、总 attach 数和最大在用 ID。
`max_id` 当前就是最大 shmid，不要假设它是密集数组长度。

## 4. 创建和 key 生命周期

```text
shmget
  -> syscall 校验 size/flags/credential
  -> registry.create_or_get_with_metadata
     -> 非 private key 命中：处理 EXCL 与 size 检查
     -> 未命中且无 CREAT：NoEntry
     -> 分配 shmid、按页取整、分配并清零物理页
     -> segments 插入；非 private 再插 key_index
```

`IPC_RMID` 立即从 `key_index` 移除并置 `marked_removed`，但已有 attachment 继续有效。此后
相同 key 可创建一个新段。旧段只有在 `marked_removed && nattch==0` 时才从主表删除并释放页。

## 5. 两阶段 shmat

必须使用 reservation 封闭 MM 锁外映射期间的生命周期窗口：

```text
registry 锁内 begin_attach(shmid)
  -> nattch +1，登记不可伪造 token，返回段快照
释放 registry 锁
  -> MM 选择/预留 VA、映射共享页、TLB 处理
成功 -> registry 锁内 finish_attach(token, task, base, readonly)
失败 -> 先撤销已建 PTE，再 cancel_attach_reservation(token)
```

每个成功的 begin 必须恰好 finish 或 cancel 一次。不能只凭 shmid 结束操作，因为同一段可有
多个并发 attach。reservation 期间 `nattch` 已增加，故并发 `IPC_RMID` 不会提前释放 pages。

若 finish 自身失败，必须先锁外 unmap，再 cancel；反过来会让最后一个引用先归零，页可能在
PTE 仍引用时返回 frame allocator。

## 6. shmdt 和退出清理顺序

正常 `shmdt` 的安全顺序是：

1. registry 锁内 `attachment_info` 复制快照，不删除 attachment；
2. 锁外用该快照 `unmap_shared_range` 并完成必要 TLB shootdown；
3. unmap 成功后 registry 锁内 `detach`，递减 `nattch`；
4. 若这是已删除段最后一个 attachment，registry 才释放物理帧。

不能先 detach 再 unmap。任务 exit/exec 也应逐 attachment 采用同样顺序；地址空间句柄已经
无效时只能在上层证明页表整体销毁已不再引用这些帧，之后才可 `drop_task`。

“地址空间销毁写回失败”类日志要先检查清理调用是否仍持有正确的旧 aspace handle，以及
清理是否发生在地址空间最终销毁之前；SHM registry 本身不应写回文件页。

## 7. fork 事务

`fork_task(parent, child)` 复制 attachment 元数据并为每段增加 `nattch`，返回 MM 映射快照。
调用者随后在子地址空间逐项映射。任一项失败时必须：

1. 撤销子地址空间中所有可能已映射的共享范围；
2. 再 `drop_task(child)` 回滚所有 child attachment/nattch；
3. 最后继续销毁子地址空间。

先回滚 registry 再 unmap 与 shmdt 一样存在 use-after-free 风险。fork 复制失败必须是全有或
全无，不能让部分 attachment 留在子表中。

## 8. 锁顺序和内存分配

全局 registry 锁保护 `segments/key_index/attachments/reservations` 的跨表不变量。锁内不得：

- 进入 address-space/MM 锁或发送 TLB IPI；
- user-copy、调度或等待；
- 格式化大型 `/proc/sysvipc/shm` 输出；
- 回调可能重新进入 SHM 的代码。

当前创建段会在 registry 操作中分配并清零多页，这是已知较重路径；若未来改为锁外分配，
必须设计 reserve/commit/cancel，保证同 key 并发创建、ID 占用和失败页释放都是事务性的。

## 9. 新增 syscall 实例：完善 `IPC_SET`

1. syscall 层复制并验证用户 `shmid_ds`；
2. registry 锁内取得只读 `ShmSegmentInfo`；
3. syscall/cred 层检查 owner/creator/root 权限；
4. 只接受允许修改的 uid/gid 与 mode 低 9 位，拒绝溢出和未知策略；
5. 调用 `update_permissions`，由实现更新 owner/mode/change_time；
6. 不修改 creator 字段、pages、size、nattch 或删除状态；
7. user-copy 失败不能留下部分元数据修改：先完整读入并校验，再一次提交；
8. 测试 owner、root、无权限、已 `IPC_RMID` 但仍 attached 的段以及并发 `IPC_STAT`。

新增 `SHM_LOCK` 时需要独立的 pin/accounting 与资源限制，不能把 `nattch` 当作锁页计数。

## 10. 泄漏与故障定位

- 段永久不释放：核对 reservations、attachments 和 `nattch` 三者；
- frame already-free：检查 MM 是否错误 dealloc 共享页，或 detach 是否重复；
- 页表仍映射但段消失：检查是否先 detach/drop_task 后 unmap；
- fork/exec 后增长：检查失败回滚与旧地址空间清理顺序；
- 同 key 查到已删除段：检查 `IPC_RMID` 是否同步移除 key_index；
- `/proc` 观测卡住：检查是否持 registry 锁格式化或复制大 pages 向量。

回归应覆盖 create/get/excl/private、两阶段 attach 每种失败点、RMID-before/after-attach、shmdt、
fork 成功/中途失败、exec/exit、多 CPU 并发和 frame 统计回落，并运行双架构 `make check`。

