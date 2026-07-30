# ipc-shm

`ipc-shm` 实现当前支持范围内的 SysV 共享内存：`shmget`、`shmat`、`shmdt` 和
`IPC_RMID`。它管理段 ID、SysV key、物理帧及 task attachment 元数据；不解析 syscall
ABI，也不直接修改用户页表。

## 分层与边界

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合 | `src/lib.rs` | 选择实现并保持 `ipc::shm::*` 对外接口稳定。 |
| API | `shm-api/api-v0/` | 标志、错误、段/附加快照；不持有状态。 |
| 实现 | `shm-impl/impl-frame/` | registry、attachment 计数和物理帧生命周期。 |
| 调用方 | `wateros-syscall/.../sys/ipc/shm.rs` | Linux ABI、用户 VA 选择、页表映射/解除映射及 errno。 |

```text
shmget ──> ShmRegistry::create_or_get ──> 分配并清零物理帧

shmat ──> begin_attach（reservation + 页快照）──> 解锁 ──> MM 预留 VA / 映射共享页
                             │失败              │成功
                             ▼                  ▼
        cancel_attach_reservation(token)   finish_attach(token)

shmdt / task exit ──> detach / drop_task ──> 调用方解除页表映射
IPC_RMID ──> 删除 key 索引；nattch 归零后才释放物理帧
```

## 核心数据与不变量

| 数据 | 所有者 | 关键语义 |
| --- | --- | --- |
| `segments: BTreeMap<ShmId, ShmSegment>` | `ShmRegistry` | 段到物理帧列表的主索引。 |
| `key_index` | `ShmRegistry` | 仅含未 `IPC_RMID` 的非 private key。 |
| `attachments` | `ShmRegistry` | task ID 到映射记录；每项对应段的一个 `nattch`。 |
| `ShmSegmentInfo` | API | `shmat` 第一阶段交给 MM 的只读段快照。 |
| `ShmAttachInfo` | API | 映射或解除映射一段用户 VA 所需的快照。 |

- `IPC_RMID` 不会立即解除仍在使用的映射；只有 `marked_removed && nattch == 0` 才回收物理帧。
- `begin_attach` 返回唯一 reservation；`finish_attach` 与 `cancel_attach_reservation` 必须使用同一
  token，MM 映射失败必须走 cancel。
- `ShmAttachInfo.pages` 的物理帧所有权仍属于 registry。MM 只能映射/取消映射，不能释放它们。
- `TaskId` 是 WaterOS 内核任务标识，不是 Linux PID 或 TID。

## 并发与 SMP

全局 `SHM_REGISTRY` 自旋锁串行化 key、段和 attachment 的关联更新。锁只保护元数据，
不覆盖 address-space/MM 锁。调用方必须遵循：**SHM registry 锁 → 释放 → MM 映射或 TLB
操作**，不能持有 registry 锁进入 MM、调度、等待或 IPI 路径。

目前 `begin_attach` 先增加 `nattch`，使并发 `IPC_RMID` 不会在页表映射期间提前释放共享帧。
完整用户态 SMP 下，MM 层仍负责页表锁、active CPU mask 和 TLB shootdown；SHM 模块不自行
发送 IPI。

## 当前限制

- 单段上限为 `MAX_SHM_SEGMENT_SIZE`（4 MiB）。
- 尚未实现完整 `shmctl(IPC_STAT/IPC_SET/SHM_LOCK...)`、权限/凭据、huge page 与 memory policy。
- `fork_task` 只复制 registry attachment；若子地址空间映射失败，调用方需要负责相应的失败恢复。

## 验证与排障

至少执行：

```sh
make -C os rv_check
make -C os la_check
```

排查泄漏时，对照 task 退出路径是否调用 `drop_task_attachments`，以及每个成功的
`begin_attach` 是否最终进入 `finish_attach` 或 `cancel_attach_reservation`。
