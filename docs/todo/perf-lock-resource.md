# 性能优化：锁竞争与资源回收热路径（进程退出 / fork / fd 表 / 注册表）

## 用途

汇总进程/任务退出清理、fork/clone 资源复制、fd 表管理、进程与 per-task 注册表的性能瓶颈与锁竞争。这是用户明确要求的「资源回收和 flush」核心区域之一。日志中 121 次 `[exit] clear_child_tid write failed` 表明退出路径被高频触发。

## 事实来源

- 代码静态链路分析；日志佐证退出路径高频。
- 关联子链路分析见 [lock-resource-subagent](2dacab6d-5c9f-4263-99c2-dd4839a74bd6)。
- 交叉参考（重点复用）：`docs/audits/lock-inventory.md`、`docs/audits/resource-inventory.md`（「跨资源生命周期钩子」表与「初步风险热点」）、`docs/audits/resources/{task-slots,file-descriptors}.md`、`docs/audits/locks/{process-registry,per-task-registries,syscall-globals}.md`。

## 覆盖范围

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/{task.rs,clone.rs,execve.rs,close.rs}`、`unix_sock.rs`、`socket_fd.rs`、`os/components/wateros-task/task-impl/impl-core/src/{process.rs,tcb.rs}`、`wateros-task/src/lib.rs`、`wateros-vfs/src/fd.rs`、`vfs-impl/impl-fd-session/src/registry.rs`、`wateros-cred/cred-impl/impl-root`。

> 注：本文档侧重「锁序 + 回收链路」；具体页缓存/帧分配器锁见 `perf-fs-vfs.md`、`perf-memory.md`，futex/signal 锁见 `perf-ipc-sync.md`。

---

## 优化点清单（按预期收益从高到低）

### L-1. 进程 reap 持 ProcessRegistry 锁内销毁地址空间 【高】

- **位置**：`os/components/wateros-task/task-impl/impl-core/src/process.rs:511-535`
- **当前实现/复杂度**：`reap_process_with_tasks` 在 ProcessRegistry 关中断 + RefCell 借用仍有效时调 `drop_user_aspace_on_task_exit(ptr)`，MM 侧 `destroy()` 递归 walk 页表（O(已映射页+页表节点)），竞争 `StackFrameAllocator`/`LockedHeap` 锁。
- **问题**：临界区 = 关中断 + Registry 独占 + 整棵地址空间 teardown，阻塞同进程/全局 MM 路径；waitpid/reap 与 exit 收尾长尾延迟源（PR-02）。
- **改进方案**：Registry 内仅 `take` aspace 指针与 task_ids，释锁后批量 `drop_user_aspace`，Registry 再短锁移除 PCB（两阶段 commit）。
- **预期收益**：高，waitpid/进程 exit 尾延迟显著下降，减少关中断窗口。
- **架构差异**：RV/LA `destroy()` 成本不同，锁序问题相同。
- **风险/依赖**：reap 失败回滚语义；MM 钩子不得回调 ProcessRegistry（当前满足，需 CI 约束）。

### L-2. 线程 exit 资源回收串行多全局锁、逐 fd close 级联 FS 锁 【高】

- **位置**：`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task.rs:226-248,795-813`
- **当前实现/复杂度**：`drop_task_runtime_resources_with_aspace` 顺序：`ShmRegistry.lock` → cwd RefCell → fd drain + 锁外逐 `handle.close()`（每个可能 sync_dirty + `GlobalFilePageCache`/`SharedFs` Mutex）→ `SOCKET_FD_REGISTRY` → `unix_sock::drop_task` → cred；pthread 压测下每线程 exit ≥5~6 次全局锁 + O(打开 fd) 次 FS 锁。
- **问题**：无批量/并行；页缓存文件 close 触发 `release_open_ref` → `purge_closed_file`；高频 exit 时 FS 锁竞争主导（对应日志 121 次 clear_child_tid 同场景）。
- **改进方案**：收集 `(handle, sidecar)` 列表统一释表锁；按 mount/backend 分组 batch flush；进程 last-thread 再 purge 页缓存；提供 `drop_task_runtime_resources_batched(task_ids[])` 供 exit_group 一次遍历。
- **预期收益**：高，pthread create/join 压测。
- **架构差异**：无。
- **风险/依赖**：CLOEXEC/execve 顺序；共享 fd 表 ref_count>0 时仅 dec ref 不 close 的逻辑保持。

### L-3. fork 路径 fd 表复制：持 fd 注册表锁期间 duplicate 全表 【高】

- **位置**：`os/components/wateros-vfs/vfs-impl/impl-fd-session/src/registry.rs:615-652`、`os/components/wateros-vfs/src/fd.rs:178-180`、`sys/clone.rs:300-307`
- **当前实现/复杂度**：两阶段，但阶段 1 仍在 `exclusive_access` 下对每个已开 fd 调 `handle.duplicate()`，O(N×duplicate)，N≤1024；`PagedFileHandle::clone` 会 `acquire_open_ref` 嵌套 page-cache `open_refs` Mutex。
- **问题**：fork 期间阻塞该 fd 表所有 syscall；duplicate 失败静默跳过致子表不完整且无错误码。
- **改进方案**：持锁仅 clone 元数据 `(fd, flags, handle_id/Arc)`，释锁后 duplicate + 安装；或 Linux 式共享 file description（fork 仅 bump ref）。
- **预期收益**：高，fork 延迟与 fd 表锁竞争；减少 duplicate 触发的页缓存锁嵌套。
- **架构差异**：无。
- **风险/依赖**：pipe/socket duplicate 语义；与 socket_fd/unix_sock copy 一致。

### L-4. unix_sock 全局 FD_TABLE 扫描式 drop/copy 【高】

- **位置**：`os/components/wateros-syscall/syscall-impl/impl-kernel/src/unix_sock.rs:86-129`
- **当前实现/复杂度**：`copy_fds_from_parent`/`drop_task` 对全局 `BTreeMap<(task_id,fd),_>` `iter()`/`keys()` 过滤，O(全局 unix 表项)而非 O(该 task fd 数)；`unregister` 移除后 `table.values().any` 再 O(表项)。
- **问题**：多进程 unix socket 时 exit/fork 成本随全局表膨胀；exit 路径额外 2~3 把 Mutex。
- **改进方案**：`BTreeMap<task_id, Vec<(fd, UnixSockRef)>>` 或 task_id 二级索引，drop O(task_fd)，fork copy 仅遍历父桶。
- **预期收益**：高，LTP/多进程 unix exit/fork 热点。
- **架构差异**：无。
- **风险/依赖**：dup 未注册 FD_TABLE（U-04）需一并修；CLONE_FILES 共享表 close 仅 unregister 当前 tid（FD-P1-04）。

### L-5. ProcessRegistry 线程查找线性扫描全进程 【高】

- **位置**：`os/components/wateros-task/task-impl/impl-core/src/process.rs:99-111,281-289,431-450`
- **当前实现/复杂度**：`lookup_task` 遍历 `processes.values()` × 每进程 tasks 线性查找 O(P×T)；`find_exited_child_process`/`has_child_process` 全表扫描；`alloc_pid` 僵尸未 reap 时循环跳过。每次 clear_child_tid、process_task_snapshot、waitpid 谓词触发。
- **问题**：进程/线程数增长后 exit/wait/clone 前置 reap 的 Registry 成本超对数预期。
- **改进方案**：维护 `TaskId → (ProcessId, slot_index)` 反向索引；find_exited_child 维护 per-parent 僵尸链表；PID 空闲位图或 generation 槽位复用。
- **预期收益**：高，Registry 读热点（尤其 `wake_clear_child_tid`）。
- **架构差异**：无。
- **风险/依赖**：spawn/fork 登记窗口 PR-01 需与索引更新原子化。

### L-6. alloc_fd 线性找空槽 + 每次 O(N) 计数 open fd 【高】

- **位置**：`os/components/wateros-vfs/vfs-impl/impl-fd-session/src/registry.rs:252-325`
- **当前实现/复杂度**：空槽 `(0..table.len()).find(|fd| table[fd].is_none())` O(N)（N≤1024）；每次 alloc/dup 前 `open_fd_count_for_task` 再 O(N) filter 计数 → 近似 O(N²)；flags/owners/ref_counts 四套 BTreeMap 各 O(log T)。
- **问题**：open/close churn 下退化。
- **改进方案**：per-owner fd 空闲位图或 `next_free_hint` + 增量维护 `open_count`；`check_nofile` 读缓存 count。
- **预期收益**：高，open/dup/pipe2/socket syscall 热路径。
- **架构差异**：无。
- **风险/依赖**：dup3 覆盖、close_range 须同步位图。

### L-7. exit_group 对兄弟线程重复 clear_child_tid / robust / shm 【高】

- **位置**：`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task.rs:251-276,216-221`、`sys/robust.rs:46-107`
- **当前实现/复杂度**：exit_group 对每个 sibling 单独 `wake_clear_child_tid`（Registry + copy_to_user + futex wake）→ `robust_exit_cleanup`（用户内存 walk + hub wake）→ `drop_task_attachments`，O(线程数×(robust 步数+锁次数))。
- **问题**：ProcessRegistry/FutexHub/ShmRegistry 反复加锁；121 次 clear_child_tid write failed 表明无效用户地址仍走完整路径（copy_to_user 失败仍 futex wake）。
- **改进方案**：合并 sibling 清理为单次 hub/registry 事务；clear_child_tid 写失败且 EFAULT 时跳过用户写仅 wake；robust 链表批处理 wake key。
- **预期收益**：高，exit_group + pthread 密集 workload。
- **架构差异**：无。
- **风险/依赖**：pthread join 内存序（`fence(SeqCst):206`）不可破坏。

### L-8. waitpid 条件谓词多次独立 ProcessRegistry 加锁 【中】

- **位置**：`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task.rs:856-873,927-934`
- **当前实现/复杂度**：每次唤醒后重新 `find_exited_child_process` + `has_child_process` 各一次 `with_process_registry`，条件闭包内再扫 O(P)。
- **问题**：TOCTOU（PR-07）；阻塞-唤醒循环放大 Registry 锁竞争。
- **改进方案**：`snapshot_wait_state(parent_pid) -> Option<exited_child>` 单次加锁；或 child_exit wait queue 事件驱动避免轮询谓词。
- **预期收益**：中，shell/测试 harness 频繁 waitpid。
- **架构差异**：无。
- **风险/依赖**：与 scheduler `TaskWaitHandle` 语义对齐。

### L-9. reap_exited_task 复合路径 ≥3 次 Registry + Scheduler 交替 【中】

- **位置**：`os/components/wateros-task/src/lib.rs:199-214`、`task-impl/impl-core/src/process.rs:454-462,645-653`、`sys/task.rs:229`
- **当前实现/复杂度**：`reap_exited_task` lookup_task → lookup_process → scheduler reap → 可能 reap_process；成员线程 reap `take_exited_member_tasks` → 每 task scheduler reap → syscall 层再 `drop_reaped_task_runtime_resources`；`sys_exit` 开头强制 reap 兄弟。
- **问题**：每次 sys_exit 放大锁与 drop 次数。
- **改进方案**：批量 reap API（Registry 一次取 id 列表 + scheduler 批量 detach）；延迟非 join 必需的资源 drop 到 idle 或 deferred list。
- **预期收益**：中，clone/exit 钩子路径。
- **架构差异**：无。
- **风险/依赖**：pthread zombie 堆积回归（task-slots §6.5）。

### L-10. registry 内部 close_slot 持借调用 handle.close() 【中】

- **位置**：`os/components/wateros-vfs/vfs-impl/impl-fd-session/src/registry.rs:117-120,509-510,676`
- **当前实现/复杂度**：`close_slot` = take + 仍持 `&mut self` 时 `handle.close()`；门面 `fd.rs::close_fd` 已 take 后释锁，但 registry 内部路径（dup3 覆盖、pipe2 回滚、execve cloexec）未统一。
- **问题**：R-PT-02，延长 fd 表不可用窗口并嵌套 FS 锁。
- **改进方案**：与 `close_fd` 一致 take → 释借 → close；close_cloexec 统一用 `take_cloexec_fds_for_task`。
- **预期收益**：中，dup3/pipe2 错误路径与 execve。
- **架构差异**：无。
- **风险/依赖**：共享表 `ensure_fd_not_io_busy` 与 close 竞态。

### L-11. flush_all 持 fd 借遍历全任务全句柄写回 【中】

- **位置**：`os/components/wateros-vfs/vfs-impl/impl-fd-session/src/registry.rs:343-352`、`os/components/wateros-vfs/src/fd.rs:110-112`
- **当前实现/复杂度**：对 `tables.values_mut()` 所有句柄 `flush()` O(全局打开 fd)；未走 `with_interrupt_disabled`（与 `with_fd_registry` 不一致）。
- **问题**：sync/fsync syscall 长时间占 fd 表；可抢占下 RefCell panic 风险。
- **改进方案**：收集句柄副本或 fd 列表后释锁 flush；或仅 sync 当前 task；补齐 interrupt guard。与 `perf-fs-vfs.md` F-7 协同。
- **预期收益**：中，全局 sync 与 bring-up flush。
- **架构差异**：无。

### L-12. fork 侧车表 socket_fd 深拷贝整表非共享 【中】

- **位置**：`os/components/wateros-syscall/syscall-impl/impl-kernel/src/socket_fd.rs:96-115`、`sys/clone.rs:304-305`
- **当前实现/复杂度**：`copy_from_parent` clone 父 owner 整个 `BTreeMap<fd, SocketRef>` O(父 inet fd 数)；线程 `share_from_parent` 仅 bump ref。
- **问题**：每 fd 复制 map 条目 + SocketRef clone；与 vfs fd duplicate 叠加。
- **改进方案**：fork 后 lazy 注册（首次 inet syscall 时 inherit）；或 fd→SocketRef 与 VfsIoHandle 同生命周期单表。
- **预期收益**：中，多 socket fork 场景。
- **架构差异**：无。
- **风险/依赖**：dup/close 旁路表同步。

### L-13. check_nofile 持 fd 借嵌套 ProcessRegistry 【中】

- **位置**：`os/components/wateros-vfs/vfs-impl/impl-fd-session/src/registry.rs:260-265`
- **当前实现/复杂度**：`check_nofile_before_open` 在 fd RefMut 内调 `task::nofile_rlimit_for_task` → `process_task_snapshot` → `with_process_registry`。
- **问题**：R-PT-04 锁序 fd→Registry；open 热路径双倍关中断/RefCell；未来 Registry 回调 open 会 panic。
- **改进方案**：spawn/fork/exec 时缓存 per-task rlimit；或先读 limit 再加 fd 借。
- **预期收益**：中，高频 open。
- **架构差异**：无。
- **风险/依赖**：setrlimit 后 cache 失效。

### L-14. execve 杀兄弟线程未清理 socket/unix 侧车表 【中（长期累积为高）】

- **位置**：`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/execve.rs:91-95`、对比 `sys/task.rs:799-800`
- **当前实现/复杂度**：`terminate_other_threads_for_exec` 后逐线程 drop cwd/fd/cred，未 `socket_fd::drop_task`/`unix_sock::drop_task`（U-08）。
- **问题**：资源泄漏 + 全局 FD_TABLE/BOUND 永久膨胀，后续 drop/copy 扫描更慢（与 L-4 叠加）。
- **改进方案**：与 `drop_task_runtime_resources_with_aspace` 对齐或统一调用。
- **预期收益**：中（长期运行累积为高）。
- **架构差异**：无。

### L-15. fork/clone 资源路径未事务化、部分失败仍留子任务 【中】

- **位置**：`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/clone.rs:287-313`
- **当前实现/复杂度**：vfs/cred/shm/unix 复制在 task 创建后串行执行；shm inherit 失败不回滚；copy_fd_table duplicate 失败静默；已有 `abort_fork_child`/`abort_clone_thread` 基础。
- **问题**：半初始化子进程/泄漏；重复 fork 浪费 Registry/fd 表扫描。
- **改进方案**：`CloneSetupGuard` 扩展覆盖 vfs+socket+shm；或 delayed enqueue（task-slots P0-1）。
- **预期收益**：中，错误路径与压测稳定性。
- **架构差异**：无。

### L-16. ASID 单调递增不回收 【低（长稳为中）】

- **位置**：`os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs:161-187`
- **当前实现/复杂度**：`NEXT_USER_ASID` fetch_add 模 16bit，不回收，复用依赖进程 reap 销毁页表。
- **问题**：长期 fork/exec 不 reap 时 TLB shootdown 频率上升；65535 后 wrap。
- **改进方案**：per-ASID generation 或空闲 ASID 栈，reap 时归还。详见 `perf-memory.md` M-2。
- **预期收益**：低（bring-up）/中（长稳）。
- **架构差异**：RV 有 ASID；LA 无 ASID 域。

### L-17. PerTaskCwdRegistry / PerTaskCredRegistry BTreeMap 查找 + cred 缺失 panic 【低】

- **位置**：`os/components/wateros-vfs/vfs-impl/impl-fd-session/src/cwd.rs`、`os/components/wateros-cred/cred-impl/impl-root/src/lib.rs:17-20,56-61`
- **当前实现/复杂度**：`effective_owner` + BTreeMap O(log T)；copy/share/drop 短临界区；cred `cred_or_panic` 缺失即 panic。
- **问题**：相对 fd/Registry 非首要热点。
- **改进方案**：TaskId 稠密时改 Vec 索引；cred 返回 Result 替代 panic。
- **预期收益**：低。
- **架构差异**：无。

### L-18. 页缓存 close 路径 purge 成本 / fd 表部分路径无 InterruptGuard 【低中】

- **位置**：`os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs:242-259`、`os/components/wateros-vfs/src/fd.rs:110-112,178-210`
- **当前实现/复杂度**：末引用 `release_open_ref` → `purge_closed_file`（已修先 sync 再 release，FD-P0-03）；`copy_fd_table_from_parent`/`drop_task_fd_table`/`flush_all_open_files` 直接 `exclusive_access()` 无 interrupt guard（对比 `with_fd_registry:37-38`）。
- **问题**：批量 close 时 purge/LRU 争用三把 Mutex；长临界区 UP 抢占 RefCell panic（R-PT-11）。
- **改进方案**：批量 close defer purge；统一经 `with_fd_registry`；消除长持借（与 L-3 合并）。
- **预期收益**：低中（含正确性）。
- **架构差异**：无。

### L-19. BufferedFileHandle fork duplicate 复制堆上整文件 【低】

- **位置**：`os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/file_handle.rs:239`
- **当前实现/复杂度**：`duplicate` = `Box::new(self.clone())` O(文件大小) 堆复制。
- **问题**：小文件 fork 多 fd 时内存/CPU 峰值（大文件已走 paged 路径）。
- **改进方案**：统一走 PagedFileHandle 或 Arc 共享 buffer。详见 `perf-fs-vfs.md` F-1。
- **预期收益**：低。
- **架构差异**：无。

### L-20. 设备注册表 / tmpfs inode / PCI BAR 无 unregister 【低（功能正确性）】

- **位置**：`docs/audits/resource-inventory.md` #34,#36-41；driver-block `register_block_device` 等
- **当前实现/复杂度**：Vec push-only；bump allocator；tmpfs inode 单调增。
- **问题**：长期 I/O 压测堆/metadata 膨胀；非 syscall 热路径。
- **改进方案**：unregister API；tmpfs inode freelist；BAR 回收。
- **预期收益**：低（正确性 > 性能）。
- **架构差异**：PCI BAR 主要 loongarch64 virt。

---

## 退出与 fork 端到端链路

退出（sys_exit 热路径）：

```
sys_exit
  → reap_exited_member_threads_runtime_resources
  → signal::on_thread_exit
  → wake_clear_child_tid (Registry + copy_to_user + futex wake)
  → robust_exit_cleanup (用户内存 walk + hub wake，≤4096 步)
  → drop_task_runtime_resources_with_aspace
        ├─ ShmRegistry.lock
        ├─ cwd RefCell
        ├─ fd drain + N×close (sync_dirty + 页缓存/SharedFs Mutex)
        ├─ SOCKET_FD_REGISTRY
        ├─ unix_sock::drop_task (FD_TABLE 全表扫描)
        └─ cred RefCell
  → task::exit_current → ProcessRegistry mark + Scheduler Exited
waitpid 收尾: reap_exited_process → reap_process_with_tasks (持 Registry 锁 MM destroy) → scheduler reap → drop_exited_task_resources
```

fork（sys_clone）：

```
fork_user_aspace → task::fork_current → signal::on_fork
  → cwd copy/share → fd copy/share + duplicate×N → socket_fd copy/share
  → unix_sock copy (全表扫描) → cred + shm
```

## 已收敛项（本次不作为首要优化）

| 项 | 说明 |
|----|------|
| 共享 fd 表 with_current_io | `fd.rs:53-81` 改 begin_shared_io + 槽位锁（FD-01 部分修复） |
| clone 线程 CLONE_FILES/FS | `clone.rs:403-414` 按 flag 分支 share/copy |
| PagedFileHandle close open_ref | `paged_handle.rs:367-368` 无论 sync 成败均 release |
| unix bind 持锁 VFS | bind VFS 移出 BOUND 临界区（U-01） |
| exit 接入 unix_sock drop_task | `task.rs:800` |

## 落地优先级建议

1. L-1 reap_process_with_tasks 锁外 drop aspace
2. L-4 unix_sock per-task 索引
3. L-3 fork fd duplicate 释锁
4. L-5 ProcessRegistry TaskId 反向索引
5. L-6 fd 空闲位图 + open_count
6. L-2 / L-7 exit 资源 batch drop / exit_group 合并
7. L-8 waitpid 单次 snapshot；L-14 execve 侧车表清理

## 风险与验证速查

> 完整口径与安全实施流程见 [`perf-risk-assessment.md`](./perf-risk-assessment.md)。**本组多数条目改动锁序/中断窗口**：单核 RefCell 伪锁下若把工作错误移出关中断守卫会触发 R-PT-11 类双重借用 panic，改前务必对照 `docs/audits/locks/*`。

| 编号 | 收益 | 风险 | 风险类型 | Flag | 关键验证 |
|------|------|------|----------|------|------|
| L-1 | 高 | 中高 | 锁序 + RefCell(R-PT-11) | 建议 | exit/wait 压测 |
| L-2 | 高 | 中 | 锁序 | 建议 | pthread 压测 |
| L-3 | 高 | 中高 | RefCell + 页缓存嵌套 | 建议 | fork 压测 |
| L-4 | 高 | 中 | 索引一致 | 否(+断言) | unix 多进程 |
| L-5 | 高 | 中 | 登记窗口原子(PR-01) | 否(+断言) | clone/wait |
| L-6 | 高 | 中 | 位图同步 | 否(+断言) | open/close churn |
| L-7 | 高 | 中 | 语义保持 | 建议 | exit_group |
| L-8 | 中 | 中 | TOCTOU | 否 | waitpid 并发 |
| L-9 | 中 | 中 | zombie 回归 | 建议 | clone/exit |
| L-10 | 中 | 中 | 竞态 | 否 | dup3/pipe2 错误路径 |
| L-11 | 中 | 中 | RefCell | 否 | 全局 sync |
| L-12 | 中 | 中 | 旁路表同步 | 建议 | socket fork |
| L-13 | 中 | 低中 | setrlimit 失效 | 否 | open + setrlimit |
| L-14 | 中 | 低 | 纯加清理 | 否 | execve |
| L-15 | 中 | 中 | 回滚一致 | 建议 | fork 失败注入 |
| L-16 | 低 | 中 | TLB 一致性（同 M-2） | 是 | 长跑 + 回绕 |
| L-17 | 低 | 低 | 行为保持 | 否 | 无 |
| L-18 | 低中 | 中 | RefCell | 否 | 批量 close |
| L-19 | 低 | 低中 | dup 语义 | 否 | fork |
| L-20 | 低 | 低中 | 回收语义 | 否 | 长跑 |

低风险可先做：L-14、L-17。高收益中险（配断言）：L-4、L-5、L-6。锁序类（配 Flag/对照审计）：L-1、L-2、L-3、L-7。

## 后续维护入口

- 改退出/fork/fd：同步 `docs/audits/resource-inventory.md` 生命周期钩子表、`docs/audits/resources/{task-slots,file-descriptors}.md`。
- 改 ProcessRegistry/调度交互：同步 `os/src/main.rs` 接线、`docs/audits/locks/process-registry.md`。
- 改 unix_sock/socket_fd：同步 `docs/audits/locks/syscall-globals.md`、`docs/audits/resources/sockets.md`。
