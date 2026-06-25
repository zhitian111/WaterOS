# 锁机制潜在问题清单

> 汇总时间：2026-06-25  
> Baseline：单核多线程（UP + 定时器抢占）  
> 来源：15 份子结构审计文档（`docs/audits/locks/`）去重合并  
> 清单索引：`docs/audits/lock-inventory.md`

---

## 1. 审计结论摘要

共审计 **35** 个带锁数据结构（15 路 subagent）。**释锁闭环**在多数路径上由 RAII（`RefMut` drop / `MutexGuard` drop）保证，未发现大规模「显式漏释锁」。

主要风险集中在三类**跨结构共性根因**：

| 根因 ID | 描述 | 影响面 |
|---------|------|--------|
| **RC-1** | 调度器 **wait/sleep 路径** `InterruptGuard` 跨越 `__switch`，唤醒后至 syscall 返回前中断仍关闭 → timer tick / 超时 / itimer 失效 | scheduler、futex、signal、pipe、poll |
| **RC-2** | **`UniprocessorSafeCell`（RefCell）在可抢占上下文无关中断保护** → `RefCell already borrowed` panic 或逻辑损坏 | 帧分配器、pipe、scheduler 引导路径 |
| **RC-3** | **`spin::Mutex` 长临界区 + 可抢占** → 持锁任务被切换后其他任务永久自旋 | klog、网络栈、块设备、页缓存、procfs |

另有两处**确定性自死锁**（与抢占无关，单线程即可触发）：

- **RC-4**：页缓存 per-file `RwLock` 在驱逐写回路径**重入**（P0，高概率卡死）
- **RC-5**：EXT4 小读缓存与块设备锁**顺序反转** AB-BA 窗口（P0）

---

## 2. 高优先级修复列表（P0）

按「易导致卡死/死锁/数据损坏」排序，合并去重后 **Top 15**：

| 优先级 | ID | 结构/模块 | 问题 | 路径/触发 | 状态 |
|--------|-----|----------|------|----------|------|
| 1 | **PC-01** | `GlobalFilePageCache` | entry `RwLock` 重入自死锁 | 缓存饱和 + read/write/flush/close 触发 LRU 驱逐写回 | **已修复** |
| 2 | **RC-1** | TaskScheduler + IPC wait | wait 路径跨 `__switch` 未释放中断 | futex wait、sigsuspend、pipe 阻塞、nanosleep | **已修复** |
| 3 | **RC-2** | `StackFrameAllocator` | 帧分配无 `InterruptGuard`，抢占 → RefCell panic | mmap/COW/lazy fault/syscall 热路径 | **已修复** |
| 4 | **IPC-01** | `KernelPipe` | `UniprocessorSafeCell` 无法保护跨任务共享 `Arc<Pipe>` | 多线程 pipe + poll/read 并发 | **已修复**（改 `spin::Mutex`） |
| 5 | **SHM-01** | `ShmRegistry` | `shmat` 锁外 MM 映射与并发 RMID/detach TOCTOU → UAF | shmat ∥ shmdt/IPC_RMID | **已修复**（`begin_attach` 占位） |
| 6 | **FS-01** | `SharedRwFs` / mount | aux RO 同块设备双 ext4 实例并发写 | bind mount RO + RW 根卷同设备 | **已收敛**（拒绝挂载 + warn） |
| 7 | **U-01** | `unix_sock` | `bind` 持锁调用 VFS mknod/metadata | AF_UNIX bind | **已修复**（VFS 先于 BOUND 锁） |
| 8 | **U-02** | `unix_sock` | 任务退出未调用 `drop_task` → BOUND/FD_TABLE 泄漏 | 进程退出后 pathname 永久占用 | **已修复** |
| 9 | **BLK-01** | EXT4 cache + BlockDevice | 写路径 cache→device 与读路径 device→cache 锁序交叉 | 多任务 ext4 并发 RW | **已收敛**（写后持 device 锁 invalidate） |
| 10 | **KLOG-01** | `KLOG` | 可抢占 + spin 锁 → 其他任务 `syslog` 永久自旋 | run_first_task 后任意 syslog | **已修复** |
| 11 | **PR-01** | `ProcessRegistry` | Spawn/Fork/Clone 登记窗口：Scheduler 先入队、Registry 后登记 | spawn/fork/clone 与 tick 交错 | **已修复** |
| 12 | **NET-01** | `NETWORK_STACK` | 全局 Mutex 覆盖整轮 smoltcp poll + VirtIO 发送 | 并发 socket syscall | **暂缓**（见 §9） |
| 13 | **PROC-01** | ProcfsLookups | 持 Mutex 执行 cwd/VFS 回调 → 自旋死锁或永久等待 | `/proc/*/cmdline` 等热路径 | **已修复** |
| 14 | **FD-01** | `PerTaskFdRegistry` | `with_current_io` 空槽窗口 + `CLONE_FILES` 共享表竞态 | poll/read ∥ dup/close 同 fd | **已修复** |
| 15 | **TRUNC-01** | `PagedFileHandle` | `truncate` 硬编码 `root_rw()`，辅助挂载路径错误 | ftruncate on tmpfs/bind mount | **已修复** |

---

## 3. 跨结构根因详解

### RC-1：wait 路径中断未释放（卡死）

**机制**：`wait_current*` / `sleep_current_for_ticks` 在函数入口创建 `InterruptGuard`，经 `__switch` 挂起后 guard 仍存活至 `take_current_wait_result` 返回。唤醒任务长时间关中断，timer 无法投递 tick。

**关联问题 ID**：scheduler §4.2；ipc-futex F-1/S-1；ipc-pipe §5.3

**收敛方向**：

- 在 `__switch` 前调用已有 `release_before_switch()`（与 `exit_current` 对齐）
- 或：wait 入队前释中断、唤醒后在 scheduler 内重新关中断

**warn 模板（临时）**：

```text
[scheduler] wait path holds InterruptGuard across __switch task_id={} wait_handle={:?}
```

---

### RC-2：UniprocessorSafeCell vs 定时器抢占（panic）

**机制**：`exclusive_access()` = `RefCell::try_borrow_mut`，失败即 panic。帧分配器、pipe state 等路径**未**配对 `InterruptGuard`，但 `trap_handler` 可在 syscall 中途调用 `schedule_tick`。

**关联问题 ID**：SFA-1；ipc-pipe §5.1；scheduler §4.1/§4.3

**收敛方向**：所有运行期 `exclusive_access` 入口加 `InterruptGuard`，或替换为 `spin::Mutex`。

---

### RC-3：spin 锁 + 抢占（活锁/卡死）

**机制**：任务 A 持 `spin::Mutex` 中被抢占；任务 B 同锁自旋；单核下 B 永不运行 → A 永不释锁。

**关联问题 ID**：KLOG #4.1；NET §5.1；DEVFS refresh；PROC-01

**收敛方向**：持锁区关中断；或缩短临界区；或换 `Mutex` + 阻塞语义（UP 下等价关中断）。

---

## 4. 分组件问题表

### 4.1 wateros-task（scheduler + process-registry）

| ID | 严重度 | 问题 | 详情文档 |
|----|--------|------|----------|
| SCH-01 | P0 | RefCell 重入 panic（未关中断路径） | `locks/scheduler.md` §4.1 |
| SCH-02 | P0 | wait 路径长时间关中断 | §4.2 |
| SCH-03 | P0 | `run_first_task` 无 InterruptGuard | §4.3 | **已修复** |
| PR-01 | P0 | Spawn/Fork/Clone 登记窗口 | `locks/process-registry.md` | **已修复** |
| PR-02 | P0 | `reap_process_with_tasks` 持锁内 MM 释放 | 同上 |
| PR-04–07 | P1 | Kill/Reap/waitpid 与 Scheduler 非原子 TOCTOU | 同上 |
| SCH-06 | P2 | RR `apply_sched_policy_change` 不迁移就绪队列 | `locks/scheduler.md` §4.6 |

### 4.2 wateros-vfs（fd / cwd / mount / page-cache / shared-fs）

| ID | 严重度 | 问题 | 详情文档 |
|----|--------|------|----------|
| R-PT-01 | P0 | `with_current_io` 共享 fd 表竞态 | `locks/per-task-registries.md` |
| R-PT-02 | P0 | `close_slot` 持借期间 `handle.close()` | 同上 |
| R-PT-03 | P1 | fork/sync 持借 duplicate/flush | 同上 |
| PC-01 | P0 | 页缓存 RwLock 重入自死锁 | `locks/page-cache.md` |
| PC-02 | P1 | mount_gen 不匹配 silent rebuild 丢脏页 | 同上 | **已收敛**（bump 前 flush + 禁止代次回退） |
| TRUNC-01 | P0 | `PagedFileHandle::truncate` 错路由 | `locks/shared-fs-handles.md` |
| FS-01 | P0 | aux RO 双 ext4 实例 | 同上 |
| MR-03 | P1 | `mount_aux_common` TOCTOU 重复挂载点 | `locks/mount-rootfs.md` | **已修复** |
| MR-04 | P1 | RootFs 多 Mutex 非原子更新 | 同上 |

### 4.3 wateros-mm + wateros-runtime

| ID | 严重度 | 问题 | 详情文档 |
|----|--------|------|----------|
| SFA-1 | P0 | 帧分配 RefCell vs 抢占 | `locks/mm-allocators.md` |
| SFA-2 | P0 | 持借期间堆分配拉长窗口 | 同上 |
| ISH-1 | P0 | 堆 Mutex 嵌套 GlobalAlloc 自旋 | 同上 | **已收敛**（重入检测 panic） |
| ISH-2 | P1 | 中断状态读取失败被 `.ok()` 吞掉 | 同上 |

### 4.4 wateros-ipc

| ID | 严重度 | 问题 | 详情文档 |
|----|--------|------|----------|
| F-1 | P0 | futex wait 继承 RC-1 | `locks/ipc-futex-signal-shm.md` |
| F-2 | P1 | FutexHub wake 持锁跨调度器 | 同上 | **已修复**（释锁后 wake） |
| SHM-01 | P0 | shmat TOCTOU UAF | 同上 |
| M-2 | P1 | fork shm 映射失败未回滚 registry | 同上 |
| IPC-01 | P0 | pipe RefCell 跨任务 | `locks/ipc-pipe.md` |
| IPC-02 | P0 | pipe `wake_one` 饿死多阻塞者 | 同上 |

### 4.5 wateros-fs + wateros-driver

| ID | 严重度 | 问题 | 详情文档 |
|----|--------|------|----------|
| PROC-01 | P0 | procfs 持锁调回调 | `locks/fs-aux.md` |
| DEVFS-01 | P0 | DEVFS refresh 长临界区 | 同上 | **已收敛**（锁外收集设备快照） |
| BLK-01 | P0 | EXT4 cache 锁序反转 | `locks/driver-block-char.md` | **已收敛** |
| BLK-02 | P0 | 块设备 Mutex 覆盖整次 VirtIO I/O | 同上 |
| UART-01 | P1 | 字符设备写路径 UART 自旋持锁 | 同上 |
| NET-01 | P0 | NETWORK_STACK 全局长持锁 | `locks/driver-network.md` |
| NET-02 | P1 | poll 反复 drive_network_stack 放大争用 | 同上 |
| PLAT-01 | P0 | RISC-V probe 三锁长临界区 | `locks/platform-probe.md` |
| PLAT-02 | P0 | 重复 init/test 非幂等重复注册设备 | 同上 |

### 4.6 wateros-syscall 全局量

| ID | 严重度 | 问题 | 详情文档 |
|----|--------|------|----------|
| U-01 | P0 | unix bind 持锁嵌套 VFS | `locks/syscall-globals.md` |
| U-02 | P0 | unix 退出未清理 | 同上 |
| U-03 | P0 | pthread clone 不同步 FD_TABLE | 同上 |
| U-04 | P1 | dup/fcntl 未同步 unix_sock 注册 | 同上 |
| C-01 | P2 | adjtimex 持锁调用 `clock_id_to_ns` | 同上 |

### 4.7 wateros-klog

| ID | 严重度 | 问题 | 详情文档 |
|----|--------|------|----------|
| KLOG-01 | P0 | 可抢占下 spin 锁卡死 | `locks/klog.md` §4.1 |
| KLOG-02 | P0 | IRQ 上下文调用 klog 互斥 | §4.2 |
| KLOG-03 | P1 | 闭包内递归日志不可重入 | §4.3 |

### 4.8 wateros-cred

| ID | 严重度 | 问题 | 详情文档 |
|----|--------|------|----------|
| R-PT-05 | P1 | cred 缺失条目 `panic!` | `locks/per-task-registries.md` |
| R-PT-09 | P3 | `AccessCheck` 恒 true（非锁 bug） | 同上 |

---

## 5. P1 及以下问题索引（节选）

| ID | 严重度 | 结构 | 摘要 |
|----|--------|------|------|
| F-2 | P1 | FutexHub | wake/requeue 持全局 futex 锁跨 `wake_one` |
| PC-02 | P1 | page-cache | mount bump 后 silent rebuild |
| PC-03 | P1 | page-cache | purge 无 flush |
| MR-02 | P1 | mount | mount 验证路径引发 Per-FS 长自旋 |
| SHARED-01 | P1 | SharedRwFs | flush 长持 FS 锁 |
| CACHE-01 | P1 | EXT4_SMALL_READ_CACHE | 全局单锁三层嵌套争用 |
| NET-03 | P2 | poll_engine | socket fd 无专用 wait，忙等 |
| MR-07–10 | P2 | mount | 热路径双锁、DEVICE_IDS 只增不减 |
| SFA-3 | P1 | frame alloc | 运行期 `init_frame_allocator` 可重置池 |
| PLAT-03 | P1 | UART_GLOBAL | 持锁阻塞读字节 |

完整条目见各 `docs/audits/locks/<name>.md` §5–§7。

---

## 6. 收敛策略与统一 warn 风格

对**锁语义未完整支持**的路径，按任务文档要求：

1. 检测不可靠前置条件（共享 fd 表、aux RO 双实例、缓存饱和驱逐等）
2. `log::warn!` 打印：`[子系统] 结构名 锁操作 函数@文件 上下文参数`
3. 返回明确错误（`EIO`/`EOPNOTSUPP`/`EBUSY`）或安全失败
4. 在本文档标注「已收敛 / 待实现」

**推荐 warn 宏占位**（实现时统一到 `wateros-base` 或 `runtime-logging`）：

```rust
macro_rules! lock_warn {
    ($struct_name:expr, $op:expr, $ctx:expr $(, $args:expr)*) => {
        log::warn!(
            "[lock-audit] struct={} op={} ctx={} {}",
            $struct_name, $op, $ctx,
            format_args!($($args)*)
        );
    };
}
```

### 本轮收敛状态

| 项 | 状态 |
|----|------|
| 审计文档 A（本文档） | ✅ 已产出 |
| 审计文档 B（`lock-coverage.md`） | ✅ 已产出 |
| 代码修复/收敛（2026-06-25 轮） | ✅ 22 项已修复/收敛，3 项暂缓（§9） |
| exports / roadmap 回填 | ✅ 高优先级项已写入 `docs/roadmap/todolist.md` |

---

## 9. 暂缓修复项（避免引入新问题）

以下问题在审计中确认存在，但本轮**未改代码**——修复需要较大重构，或当前无零副作用补丁。

| ID | 原因 | 建议后续 |
|----|------|----------|
| **NET-01** | 拆分 `NETWORK_STACK` 锁需重划 smoltcp 与 VirtIO 边界；草率拆锁易引入包丢失或双 poll | 锁外 I/O 或 per-socket 状态机，单独设计评审 |
| **PLAT-01/02** | 平台 probe 三锁 / 重复 init 面大，需 probe 重构与 init 幂等 | 单独平台层评审 |
| **F-2**（requeue） | `requeue_to` 仍持 futex 全局锁跨调度 | 与 `wake` 同模式释锁后操作 |
| **U-03** | pthread clone 未同步 `FD_TABLE` | **已修复**（`clone.rs` 增加 `copy_fds_from_parent`） |
| **IPC-02** | pipe `wake_one` 饿死 | **已修复**（改为 `wake_all`） |

### 第三轮（FD-01 完整修）代码变更摘要

| 组件 | 变更 |
|------|------|
| `impl-fd-session/registry` | 共享表 `io_inflight` + `io_slot_locks`；`begin/end_shared_io_for_task` |
| `vfs/fd.rs` | 共享表 I/O 不 take 槽位；`with_fd_registry` 关中断 |
| `vfs-api` | 新增 `VfsError::Busy` → `EBUSY` |
| `sys/clone.rs` | `CLONE_FILES`/`CLONE_FS` 控制 share vs copy |

### 本轮（第二轮）代码变更摘要

| 组件 | 变更 |
|------|------|
| `scheduler-impl/*` | `create_*` / `enqueue_ready_task` 拆分；`run_first_task` 加 `InterruptGuard` |
| `wateros-task` | spawn/fork/clone：先登记 ProcessRegistry 再入队 |
| `mount_table` | `mount_aux_common` 检查+push 同临界区；bump 前 `reset_file_page_cache` |
| `impl-page-cache` | `global_cache` 禁止代次回退；升级代次 warn |
| `impl-ext4/rw` | 写路径在持 device 锁期间 invalidate 小读缓存 |
| `ipc-futex/hub` | `wake`/`wake_all` 释锁后调度 |
| `devfs-impl-kernel` | `refresh` 锁外收集 block/char 设备 |
| `runtime-heap-allocator` | 堆分配重入深度检测 panic |

### 第一轮代码变更摘要

| 组件 | 变更 |
|------|------|
| `scheduler-impl/*` | wait/sleep：`release_before_switch` + `finish_wait_after_switch` |
| `impl-page-cache` | read/write/flush 不持 entry 锁调用 `install_page`；`writeback` 显式 `logical_size` |
| `impl-stack` | 帧分配器 `FrameAllocatorInterruptGuard` |
| `klog-ringbuf` | `KlogInterruptGuard` 包裹 `with`/`iter_from` |
| `ipc-pipe/ringbuf` | `PipeState` → `spin::Mutex`；`wake_all` |
| `ipc-shm` + `sys/shm.rs` | `begin_attach` / `finish_attach` / `cancel_attach_reservation` |
| `unix_sock` | `bind` 锁序；`drop_task` 走 `unregister` |
| `sys/task.rs` | 退出路径调用 `unix_sock::drop_task` |
| `sys/clone.rs` | 线程 clone 继承 `FD_TABLE` |
| `procfs-impl` | 锁外调用 lookup 回调 |
| `paged_handle` | `truncate` 经 `resolve_route` |
| `rootfs-impl` | 同设备 aux RO 拒绝双 ext4 |

---

## 7. 建议修复顺序（供实现阶段）

1. **PC-01** 页缓存 RwLock 重入（确定性单线程死锁，易复现）
2. **RC-1** 调度器 wait 释中断（解除 futex/pipe/poll 大面积卡死）
3. **RC-2 / SFA-1** 帧分配器关中断（syscall/MM 热路径 panic）
4. **IPC-01** pipe 改 `spin::Mutex`（多线程 pipe 基础可用性）
5. **SHM-01 / U-02 / U-03** 语义缺口（UAF 与资源泄漏）
6. **KLOG-01** syslog 持锁策略（低成本，减少干扰调试）
7. 其余 P0 按测例失败栈就近修复

---

## 8. 参考文档

- 清单：`docs/audits/lock-inventory.md`
- 单结构审计：`docs/audits/locks/*.md`（15 份）
- 任务说明：`docs/tasks/audit_lock_mechanisms.md`
