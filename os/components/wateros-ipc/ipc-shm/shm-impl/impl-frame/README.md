# Frame-backed SysV SHM 实现手册

[IPC 总览](../../../README.md) · [MM API](../../../../wateros-mm/mm-api/api-v0/README.md)

该实现拥有 SysV SHM 的 key/id/权限/时间/attachment 元数据与普通物理帧。syscall/MM 选择用户 VA、安装 external PTE、撤销映射和 TLB；registry 不保存地址空间 handle。`ShmSegment.pages` 是唯一物理 owner，所有快照里的 PPN 只是映射清单，调用方绝不能 dealloc。

## Registry 状态

`ShmRegistry` 的全部字段由一个全局 spin Mutex 保护：

- `segments: BTreeMap<ShmId, ShmSegment>` 是 owner 主表；
- `key_index` 只含未 RMID 且非 IPC_PRIVATE 的 key；
- `attachments: TaskId -> Vec<(shmid,base,size,readonly)>`；
- `attach_reservations: token -> shmid` 保护映射中窗口；
- 两个 next id wrapping 线性探测，跳过 0。

`IPC_RMID` 立即删 key index并标记 removed，已有 attachment/进行中 reservation 继续有效；只有 `marked_removed && nattch==0` 才删除 segment、逐页归还 frame。`nattch` 同时计已提交映射和进行中的 begin_attach。

`create_or_get` 对非 private key：存在时 CREXCL→Exists，请求 size 大于现有段→Invalid，否则返回旧 id；不存在且无 IPC_CREAT→NoEntry。新段 size 必须非零且不超过 `MAX_SHM_SEGMENT_SIZE`，存储 size 为页向上对齐值，mode 只取 flags 低 9 位。

## 分配与 heap 风险

`alloc_segment_pages` 当前循环：frame_alloc → 恒等映射整页清零 → `pages.push`。frame OOM 会回收先前页，但 `Vec` 没有先 `try_reserve_exact(count)`；某次 push 扩容失败会触发 kernel allocation error，刚取 frame也来不及回滚。修复应先 checked 计算页数、fallible reserve 完整页号容量，再分配；最好用 RAII frame list 让任意提前返回自动释放。

`create_or_get` 在持全局 SHM registry 锁时分配并清零整个段，大段可长时间阻塞所有 shmget/shmat/shmdt/exit，并形成 SHM→frame allocator 锁序。可靠事务应：锁内检查 key并登记 Creating token → 解锁分配/清零 → 重锁验证 key/token并提交；并发 create 的等待/失败语义要明确。

`segment_info`、`attachment_info`、`task_attachments`、`segment_infos` 都 clone 整个 pages Vec，且大多在 registry 锁内用不可失败分配。`sys_shmat` 目前先 `segment_info` 做权限，再 `begin_attach` 内部又 clone 一次，形成重复临时页号数组。大 SHM 下会推高固定 kernel heap。可把权限检查与 begin 合并，并用 `Arc<[PhysPageNum]>`/引用计数 backing 避免每次复制。

## 两阶段 attach

```text
begin_attach
  -> 校验段，nattch++，登记不可伪造 token，返回 pages 快照
  -> 解锁 registry
  -> MM 预留 VA，安装 readonly/RW external PTE，flush TLB
  -> 成功 finish_attach；失败先 unmap 已装 PTE，再 cancel token
```

把“正在 MM 映射”计入 nattch 可防并发 RMID 释放帧。token 字段私有，finish/cancel 校验 `(id,shmid)`，必须恰好一次。

`finish_attach` 先构造 pages clone，再向 task attachment Vec push，最后删除 reservation；clone/push 都可能 heap panic。若改成 fallible 操作，失败必须保留 reservation 供 caller cancel，不能先删 token或少减 nattch。

`SHM_REMAP` 可能先破坏目标旧映射，整个流程不是天然事务，MM 手册中的 FIXED 风险同样适用。

detach 正确链是 registry `attachment_info` 快照 → 解锁 → `unmap_shared_range` → 重锁 `detach`。若 unmap 失败，attachment/nattch 保留；若 detach 在成功 unmap 后异常失败，则 registry 与页表不一致，源码假定同一 task 不会并发修改。

`detach_attachment` 与 cancel 使用 `saturating_sub`，可能掩盖计数下溢；正常不变量应使用 checked_sub 并记录严重错误，而不是把 0 保持为 0 后继续释放段。

## fork、exec、exit

`fork_task` 在 registry 锁内 clone 父 attachment、逐段 nattch++、向 child Vec push并返回 pages 快照；遇单段 nattch overflow只跳过该 attachment，不使整个 fork 失败，可能导致子页表与 SHM registry 不完整。Vec/page clone也不可失败。

syscall 随后在 child aspace逐个 replace external mapping；任一失败会 unmap 所有返回项并 `drop_task(child)` 回滚 nattch。若 registry 阶段 heap panic或静默 skip，回滚无法完整执行。应把它改成可失败 reservation transaction，全部元数据预留成功后一次提交。

exec/exit 对旧 aspace先 snapshot attachments，逐个成功 unmap 后 detach。若 aspace handle=0，当前直接 drop_task 元数据；这只在地址空间已经由其它路径彻底销毁、external PTE不会再引用 frame 时安全。

## 删除与锁序

`remove_segment` 在 SHM registry 锁内遍历所有 pages并调用 frame_dealloc_result，失败只 warning。这可形成很长的双锁路径；若 allocator 日志/诊断反向取 SHM，会死锁。更安全是锁内 remove 出 owned segment，解锁后由 RAII owner归还 frames，但必须确保没有新快照可获得。

更新 owner/mode 的权限判断在 syscall层，registry只提交低 9 位。统计的 `attached_count` 包含 reservation；`segment_infos` 用于 proc 格式化，必须拿到快照后解锁再 format/copy user。

## 新功能实例：SHM_LOCK

增加 shmctl SHM_LOCK/UNLOCK 时，先在 API/segment 增加明确 lock state；syscall验证 owner/CAP_IPC_LOCK与资源限制；registry事务修改状态。由于 frames本来常驻，当前兼容实现可能只是记录状态，必须明确不改变 nattch/RMID owner，不能伪称支持换页锁定。

## 回归清单

- IPC_PRIVATE、key create/excl、已存在 size、RMID 后同 key新建；
- 0/1/页边界/MAX/MAX+1/溢出 size；
- frame OOM和 pages Vec reserve OOM，frame/heap均恢复；
- 大段创建期间其它 SHM 操作的锁延迟；
- attach token成功/cancel/重复/错配，映射中并发 RMID；
- readonly/exec权限、SHM_RND/REMAP、部分映射失败；
- detach unmap失败/成功、重复 base、nattch checked下溢；
- fork完整成功与第 N 个映射失败，exec/exit/aspace=0；
- 最后 attachment释放后 key/segment/pages和 frame基线；
- 大段反复 stat/attach/proc snapshot 的 kernel heap峰值与回落。
