# ipc-shm

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [wateros-ipc](../readme.md)

`ipc-shm` 实现 WaterOS 当前支持范围内的 SysV 共享内存。它管理段 ID、SysV key、物理帧和
task attachment 元数据，但不解析 syscall ABI，也不直接修改用户页表。

## 模块分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合层 | `src/lib.rs` | 选择 frame 实现并导出 API、registry 和 reservation。 |
| SHM API | `shm-api/api-v0/` | 定义标志、错误、段快照和 attachment 快照。 |
| SHM 实现 | `shm-impl/impl-frame/` | 管理 registry、segment、attachment、reservation 和物理页。 |
| 系统调用层 | `wateros-syscall/.../sys/ipc/shm.rs` | 解析 Linux ABI、选择用户 VA、修改页表并映射 errno。 |
| MM 层 | `wateros-mm` | 映射/解除共享物理页、维护 active CPU 和执行 TLB shootdown。 |

实现文件按职责拆分如下：

| 文件 | 内容 |
| --- | --- |
| `shm-api/api-v0/src/lib.rs` | ShmId、标志、错误和公开快照。 |
| `shm-impl/impl-frame/src/state.rs` | 内部 segment、attachment 和 reservation 状态。 |
| `shm-impl/impl-frame/src/allocation.rs` | 大小按页对齐、物理页分配和清零。 |
| `shm-impl/impl-frame/src/registry.rs` | ShmRegistry 与两阶段 attach。 |
| `shm-impl/impl-frame/src/global.rs` | 全局 SHM_REGISTRY facade。 |

## 实现说明

- 当前支持 `shmget`、`shmat`、`shmdt` 和 `shmctl(IPC_RMID)` 的核心语义。
- SHM registry 拥有共享段的物理页。MM 只建立或删除 PTE，不能把这些页交给普通 frame
  allocator 回收。
- attachment 使用 WaterOS `TaskId` 标识拥有映射的任务，不使用 Linux PID/TID 替代。
- `shmat` 使用 reservation 分成“锁内登记”和“锁外映射”两阶段，避免持 SHM 锁进入 MM。
- `IPC_RMID` 只删除 key 可见性并标记段；已有映射继续使用，直到最后一个 attachment 消失。
- SHM registry 锁不覆盖 address-space/MM 锁，也不能跨越用户复制、调度、等待或 TLB IPI。
- 页表并发、active CPU mask 和 TLB shootdown 属于 MM 层，SHM 模块不自行发送 IPI。
- 当前单段上限为 4 MiB；完整权限、`IPC_STAT/IPC_SET`、SHM_LOCK、huge page 和 memory
  policy 尚未实现。

## 调用链路

创建或查询共享段：

```text
sys_shmget(key, size, flags)
  -> syscall 层校验 Linux 参数
  -> ShmRegistry::create_or_get
  -> 查找未删除的 key，或分配新的 ShmId
  -> 分配、清零共享物理页并登记 segment
  -> 返回 ShmId
```

附加共享段：

```text
sys_shmat(shmid, address, flags)
  -> ShmRegistry::begin_attach(task_id, shmid, ...)
  -> 锁内增加 nattch，创建唯一 reservation，并复制物理页快照
  -> 释放 SHM registry 锁
  -> MM 选择/预留用户 VA 并映射共享页
  -> 成功：finish_attach(token, base)
  -> 失败：cancel_attach_reservation(token)
```

解除映射和删除：

```text
shmdt / task exit
  -> registry 取得 ShmAttachInfo 并删除 attachment
  -> 锁外由 MM 解除用户页表映射

IPC_RMID
  -> 删除 key_index 并标记 segment removed
  -> nattch > 0 时保留页面
  -> nattch == 0 时删除 segment 并释放物理页
```

## ShmSegment实现功能

内部 segment 状态位于 `shm-impl/impl-frame/src/state.rs`。

- 保存 ShmId、可选 SysV key、请求大小、实际页数和物理页列表。
- 保存创建标志、attachment 计数和 `marked_removed` 状态。
- 未删除的非-private key 同时存在于 `key_index`；`IPC_RMID` 后立即从该索引移除。
- 物理页在段创建时分配并清零，在段真正销毁前始终由 segment 持有。
- `marked_removed && nattch == 0` 是释放段及物理页的必要条件。

`ShmSegmentInfo` 是交给 syscall/MM 的只读段快照；它不转移物理页所有权。

## ShmAttachment实现功能

attachment 和 reservation 状态位于 `state.rs`，两阶段操作位于 `registry.rs`。

- attachment 记录 task、ShmId、用户映射基址、长度和映射属性。
- `begin_attach` 先增加 nattch 并返回唯一 token，使并发 `IPC_RMID` 无法在 MM 映射期间提前
  释放物理页。
- `finish_attach` 只能消费匹配 token，并把已完成映射登记到 task attachment 表。
- `cancel_attach_reservation` 撤销失败映射的临时 nattch，并在满足删除条件时回收段。
- `detach`、`drop_task` 和任务退出清理会移除 attachment 并返回 MM 解除映射需要的快照。
- fork 可以复制父任务的 attachment 登记；若子页表映射失败，调用方必须执行对应回滚。

每个成功的 `begin_attach` 都必须进入 finish 或 cancel，不能遗漏 token，也不能重复消费。

## ShmRegistry实现功能

`ShmRegistry` 定义在 `shm-impl/impl-frame/src/registry.rs`，由 `global.rs` 中的
`SHM_REGISTRY` 锁保护。

- `segments` 是 `ShmId -> segment` 主表。
- `key_index` 将未删除的非-private SysV key 映射到 ShmId。
- `attachments` 按 task 维护现有用户映射。
- reservation 表协调锁内元数据与锁外 MM 映射的提交/回滚。
- 提供 create/get、remove、begin/finish/cancel attach、detach、task cleanup 和 fork 复制操作。
- 所有跨 MM 的调用都先生成独立快照并释放 registry 锁。

## SHM聚合层实现功能

`ipc-shm/src/lib.rs` 负责导出 `api-v0` 和 `impl-frame`：

- 对外提供 ShmId、ShmError、标志、ShmSegmentInfo 和 ShmAttachInfo。
- 保留 `ipc::shm::registry().lock()` 的现有调用接口，并导出 ShmRegistry 与 reservation 类型。
- 调用方可以在 registry 锁内完成短小元数据操作，但必须在访问 MM、用户地址或调度器之前
  释放该锁。

排查共享内存泄漏时，应对照任务退出是否调用 attachment cleanup，以及每个 begin-attach 是否
有唯一的 finish/cancel；排查页表错误时则检查 MM 映射和 TLB 路径，不应让 SHM registry
回收仍被映射的设备页或共享页。
