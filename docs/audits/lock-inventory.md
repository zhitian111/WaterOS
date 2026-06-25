# 带锁数据结构清单 & 资源分配及回收链路

> 生成时间：2026-06-25（第二轮盘点）  
> 搜索范围：`os/components/**`（含平台驱动 probe 静态量）  
> Baseline：单核多线程（UP + 定时器抢占）；`UniprocessorSafeCell` = `RefCell` 运行时独占借用；`spin::Mutex` / `spin::RwLock` = 自旋锁（RAII 释锁）

---

## 一、带锁数据结构总表

| # | 数据结构名称 | 主要文件 | 锁类型 | 预估复杂度 | Subagent 分组 |
|---|-------------|---------|--------|-----------|--------------|
| 1 | `UniprocessorSafeCell<T>` | `wateros-base/src/sync/uniprocessor.rs` | `RefCell` 运行时借用（单核伪锁） | 低（原语） | 并入各使用者审计 |
| 2 | `ProcessRegistry` | `wateros-task/task-impl/impl-core/src/lib.rs` | `UniprocessorSafeCell` | 中 | `process-registry` |
| 3 | `RoundRobinScheduler`（含 `TaskRegistry` + `WaitQueues` + 就绪队列） | `wateros-task/task-scheduler/scheduler-impl/impl-round-robin/` | `UniprocessorSafeCell` | 高 | `scheduler` |
| 4 | `MultiClassScheduler`（含 `TaskRegistry` + `WaitQueues` + 多级就绪队列） | `wateros-task/task-scheduler/scheduler-impl/impl-multi-class/` | `UniprocessorSafeCell` | 高 | `scheduler` |
| 5 | `PerTaskFdRegistry` | `wateros-vfs/src/fd.rs`, `vfs-impl/impl-fd-session/src/registry.rs` | `UniprocessorSafeCell` | 高 | `per-task-registries` |
| 6 | `PerTaskCwdRegistry` | `wateros-vfs/src/cwd.rs` | `UniprocessorSafeCell` | 中 | `per-task-registries` |
| 7 | `PerTaskCredRegistry` | `wateros-cred/cred-impl/impl-root/src/lib.rs` | `UniprocessorSafeCell` | 中 | `per-task-registries` |
| 8 | `StackFrameAllocator` | `wateros-mm/mm-frame-alloctor/frame-alloctor-impl/impl-stack/src/lib.rs` | `UniprocessorSafeCell` | 中高 | `mm-allocators` |
| 9 | `InterruptSafeLockedHeap` / `LockedHeap` | `wateros-runtime/runtime-heap-allocator/src/lib.rs` | `spin::Mutex`（`LockedHeap` 内部）+ 中断屏蔽守卫 | 高 | `mm-allocators` |
| 10 | `AUX_MOUNTS` / `DEVICE_IDS`（挂载表） | `wateros-vfs/vfs-impl/impl-fs-bridge/src/mount_table.rs` | `spin::Mutex` ×2 | 中 | `mount-rootfs` |
| 11 | `ROOT_FS` / `ROOT_RW_FS` / `ROOT_DEV_PATH` / `ACTIVE_FS_IMPL` | `wateros-fs/fs-rootfs/rootfs-impl/impl-kernel/src/lib.rs` | `spin::Mutex` ×4 | 中 | `mount-rootfs` |
| 12 | `GlobalFilePageCache`（`state` / `files` / `open_refs` + per-file `FileEntryInner`） | `wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs` | `spin::Mutex` ×3 + `spin::RwLock`（per-file） | 高 | `page-cache` |
| 13 | `SharedFs` / `SharedRwFs`（`Arc<Mutex<LocalFs>>` 实例锁） | `wateros-fs/fs-api/api-v0/`, `vfs-impl/impl-fs-bridge/`, `fs-impl/impl-ext4*` | `spin::Mutex`（per-FS 实例） | 高 | `shared-fs-handles` |
| 14 | `FutexHub` / `FutexTables` | `wateros-ipc/ipc-futex/futex-impl/impl-task/src/hub.rs` | `spin::Mutex` | 高 | `ipc-futex-signal-shm` |
| 15 | `SignalRegistry` | `wateros-ipc/ipc-signal/src/lib.rs` | `spin::Mutex` | 高 | `ipc-futex-signal-shm` |
| 16 | `ShmRegistry` | `wateros-ipc/ipc-shm/src/lib.rs` | `spin::Mutex` | 中 | `ipc-futex-signal-shm` |
| 17 | `KernelPipe` / `PipeState` | `wateros-ipc/ipc-pipe/pipe-impl/impl-ringbuf/src/kernel_pipe.rs` | `spin::Mutex`（已自 `UniprocessorSafeCell` 迁移） | 中 | `ipc-pipe` |
| 18 | `DEVFS`（`DevFsImpl`） | `wateros-fs/fs-devfs/devfs-impl/impl-kernel/src/lib.rs` | `spin::Mutex` | 中 | `fs-aux` |
| 19 | `DEV_NODES` | `wateros-fs/fs-impl/impl-devfs/src/lib.rs` | `spin::Mutex` | 低 | `fs-aux` |
| 20 | `ARGV_LOOKUP` / `EXE_LOOKUP` / `MOUNT_LOOKUP` | `wateros-fs/fs-procfs/procfs-impl/impl-kernel/src/lib.rs` | `spin::Mutex` ×3 | 低 | `fs-aux` |
| 21 | `EXT4_SMALL_READ_CACHE` | `wateros-fs/fs-impl/impl-ext4/src/rw.rs` | `spin::Mutex` | 中 | `fs-aux` |
| 22 | `BLOCK_DEVICES` | `wateros-driver/driver-block/block-api/api-v0/src/lib.rs` | `spin::Mutex` | 中 | `driver-block-char` |
| 23 | `CachingBlockDevice`（`Arc<Mutex<dyn BlockDevice>>` 包装） | `wateros-driver/driver-block/block-impl/impl-block-cache/` | `spin::Mutex`（per-device） | 中 | `driver-block-char` |
| 24 | `CHARACTER_DEVICES` | `wateros-driver/driver-character/character-api/api-v0/src/lib.rs` | `spin::Mutex` | 低中 | `driver-block-char` |
| 25 | `NETWORK_DEVICES` | `wateros-driver/driver-network/network-api/api-v0/src/lib.rs` | `spin::Mutex` | 中 | `driver-network` |
| 26 | `NETWORK_STACK` | `wateros-driver/driver-network/src/lib.rs` | `spin::Mutex` | 高 | `driver-network` |
| 27 | `SocketHandle.inner` | `wateros-driver/driver-network/src/socket_handles.rs` | `Arc<Mutex>`（per-handle） | 低中 | `driver-network` |
| 28 | `DEVICE_INFOS` / `VIRTIO_BLK_MMIO` / `VIRTIO_NET_MMIO` | `wateros-driver/driver-impl/impl-qemu-riscv64-opensbi/src/lib.rs` | `spin::Mutex` ×3 | 中 | `platform-probe` |
| 29 | `VIRTIO_BLK_PCI` / `VIRTIO_NET_PCI` | `wateros-driver/driver-impl/impl-qemu-loongarch64-virt/src/lib.rs` | `spin::Mutex` ×2 | 中 | `platform-probe` |
| 30 | `UART_GLOBAL` | `wateros-driver/driver-impl/impl-qemu-loongarch64-virt/src/uart.rs` | `spin::Mutex` | 低 | `platform-probe` |
| 31 | `SOCKET_FD_REGISTRY` | `wateros-syscall/syscall-impl/impl-kernel/src/socket_fd.rs` | `spin::Mutex` | 中 | `syscall-globals` |
| 32 | `FD_TABLE` / `BOUND` / `UnixSockInner` | `wateros-syscall/syscall-impl/impl-kernel/src/unix_sock.rs` | `spin::Mutex` ×3 | 高 | `syscall-globals` |
| 33 | `TIMES`（stat 时间戳表） | `wateros-syscall/syscall-impl/impl-kernel/src/stat_times.rs` | `spin::Mutex` | 低 | `syscall-globals` |
| 34 | `TIMEX_STATE` | `wateros-syscall/syscall-impl/impl-kernel/src/sys/clock.rs` | `spin::Mutex` | 低中 | `syscall-globals` |
| 35 | `KLOG` / `KlogRingbufInner` | `wateros-klog/klog-impl/klog-ringbuf/src/lib.rs` | `spin::Mutex` | 中 | `klog` |

### 未纳入（原子变量，非显式锁）

| 名称 | 文件 | 说明 |
|------|------|------|
| `REALTIME_OFFSET_NS` | `wateros-platform/src/wall_clock.rs` | `AtomicI64`，Relaxed 序；与 `TIMEX_STATE` 交叉关注 |

---

## 二、资源分配及回收链路清单

> **术语**：本任务语境下「资源分配」= 获取独占访问（`exclusive_access` / `.lock()`）；「回收」= RAII guard drop 或显式提前 drop。  
> **释锁模型**：`RefMut` / `MutexGuard` / `RwLockReadGuard` / `RwLockWriteGuard` 均在作用域结束时自动回收。

### 2.1 原语层

| 原语 | 分配入口 | 回收出口 | 失败形态 |
|------|---------|---------|---------|
| `UniprocessorSafeCell::exclusive_access` | 任意调用方 | `RefMut` drop | 重入 → panic `RefCell already borrowed` |
| `spin::Mutex::lock` | 任意调用方 | `MutexGuard` drop | 持锁任务被切换 → 其他任务永久自旋 |
| `spin::RwLock::read/write` | 页缓存 per-file | guard drop | 写锁重入 → 自死锁 |

### 2.2 按 Subagent 分组的持锁/释锁链路

#### `scheduler` — #3, #4

| 链路 | 分配（持锁） | 回收（释锁） | 上游触发 |
|------|------------|------------|---------|
| 调度器全局 | `with_scheduler` → `exclusive_access()` | 闭包返回 / `RefMut` drop | syscall、trap tick、`yield_now`、IPC wake |
| 引导路径 | `init_scheduler` / `run_first_task` 直接 `exclusive_access` | 函数返回 | `main` bring-up |
| wait/sleep | `wait_current*` / `sleep_current_for_ticks` 两次 `with_scheduler` + 可能 `__switch` | 第二次 `with_scheduler` 结束；**须**在 switch 前释 `InterruptGuard` | futex、pipe、poll、nanosleep |
| 跨结构 | 常与 `ProcessRegistry`（fork/clone 序）、`FutexHub`（wake）交错 | — | 见 RC-1 |

#### `process-registry` — #2

| 链路 | 分配 | 回收 | 上游触发 |
|------|------|------|---------|
| 进程表 | `with_registry` → `exclusive_access` | 闭包结束 | fork/clone/exit/waitpid |
| 与调度器 | spawn 后 scheduler 先入队、registry 后登记（窗口） | — | spawn/fork/clone |

#### `per-task-registries` — #5, #6, #7

| 链路 | 分配 | 回收 | 上游触发 |
|------|------|------|---------|
| FD 表 | `fd::registry().exclusive_access()` / `with_current_io` | `RefMut` drop | open/close/dup/poll/read/write |
| CWD | `cwd::registry().exclusive_access()` | drop | chdir/openat/cwd 相对路径 |
| Cred | `cred` registry `exclusive_access` | drop | setuid/getuid/faccessat |

#### `mm-allocators` — #8, #9

| 链路 | 分配 | 回收 | 上游触发 |
|------|------|------|---------|
| 帧分配器 | `frame_allocator_cell().exclusive_access()` + `InterruptGuard` | drop | page fault、mmap、COW、内核栈 |
| 堆 | `LockedHeap::lock` + 中断守卫 | guard drop | `alloc`/`dealloc`、驱动 DMA |

#### `mount-rootfs` — #10, #11

| 链路 | 分配 | 回收 | 上游触发 |
|------|------|------|---------|
| 辅助挂载 | `AUX_MOUNTS.lock()` / `DEVICE_IDS.lock()` | guard drop | mount/bind、路径解析 |
| 根 FS | `ROOT_FS` / `ROOT_RW_FS` / `ACTIVE_FS_IMPL` 各 `.lock()` | guard drop | 启动挂载、FS 切换 |

#### `page-cache` — #12

| 链路 | 分配 | 回收 | 上游触发 |
|------|------|------|---------|
| 全局 | `GLOBAL_CACHE` → `state`/`files`/`open_refs` Mutex | guard drop | open/read/write/close/mmap |
| per-file | `FileEntryInner` RwLock read/write | guard drop | 页读写、驱逐写回（**禁重入写锁**） |
| 锁序 | 通常 `state` → `files` → entry RwLock | 逆序 drop | 与 `SharedRwFs` 嵌套 |

#### `shared-fs-handles` — #13

| 链路 | 分配 | 回收 | 上游触发 |
|------|------|------|---------|
| FS 实例 | `SharedFs::lock` / `SharedRwFs::lock`（`Arc<Mutex<Ext4>>`） | guard drop | VFS bridge、ext4 rw/ro |
| 双实例 | 同块设备 RO+RW 两 Mutex | — | bind mount（已收敛拒绝） |

#### `ipc-futex-signal-shm` — #14, #15, #16

| 链路 | 分配 | 回收 | 上游触发 |
|------|------|------|---------|
| Futex | `FutexHub` Mutex | guard drop | futex wait/wake |
| Signal | `SIGNAL_REGISTRY.lock()` | guard drop | kill/rt_sigaction/sigsuspend |
| SHM | `SHM_REGISTRY.lock()` | guard drop | shmget/shmat/shmdt；`begin_attach` 占位 |

#### `ipc-pipe` — #17

| 链路 | 分配 | 回收 | 上游触发 |
|------|------|------|---------|
| Pipe 状态 | `PipeState` `Mutex::lock` | guard drop | read/write/poll；阻塞 wait 经 scheduler |

#### `fs-aux` — #18–#21

| 链路 | 分配 | 回收 | 上游触发 |
|------|------|------|---------|
| devfs | `DEVFS` / `DEV_NODES` | guard drop | mknod、设备节点 |
| procfs | `ARGV/EXE/MOUNT_LOOKUP` | guard drop | `/proc` 读；**禁**持锁调 VFS |
| ext4 cache | `EXT4_SMALL_READ_CACHE` | guard drop | 小读路径；与 `BLOCK_DEVICES` 锁序敏感 |

#### `driver-block-char` — #22–#24

| 链路 | 分配 | 回收 | 上游触发 |
|------|------|------|---------|
| 块设备表 | `BLOCK_DEVICES.lock()` | guard drop | 注册、读写 |
| 缓存包装 | per-device `Arc<Mutex<BlockDevice>>` | guard drop | ext4、块 IO |
| 字符设备 | `CHARACTER_DEVICES.lock()` | guard drop | `/dev` 读写 |

#### `driver-network` — #25–#27

| 链路 | 分配 | 回收 | 上游触发 |
|------|------|------|---------|
| 设备表 | `NETWORK_DEVICES.lock()` | guard drop | 注册、probe |
| 协议栈 | `NETWORK_STACK.lock()`（长临界区） | guard drop | poll、send、recv syscall |
| socket | per-handle `inner.lock()` | guard drop | socket fd 操作 |

#### `platform-probe` — #28–#30

| 链路 | 分配 | 回收 | 上游触发 |
|------|------|------|---------|
| RISC-V probe | `DEVICE_INFOS` / VirtIO MMIO 表 | guard drop | 启动 probe（单线程） |
| LoongArch probe | PCI VirtIO 表 / `UART_GLOBAL` | guard drop | 启动、console |

#### `syscall-globals` — #31–#34

| 链路 | 分配 | 回收 | 上游触发 |
|------|------|------|---------|
| socket fd | `SOCKET_FD_REGISTRY.lock()` | guard drop | socket syscall |
| unix sock | `FD_TABLE` / `BOUND` / `UnixSockInner` | guard drop | bind/connect/sendmsg |
| 元数据 | `TIMES` / `TIMEX_STATE` | guard drop | stat、adjtimex |

#### `klog` — #35

| 链路 | 分配 | 回收 | 上游触发 |
|------|------|------|---------|
| 内核日志环 | `KLOG.lock()` + `InterruptGuard` | guard drop | `syslog`、内核 `klog_*!` |

### 2.3 跨结构锁序依赖（审计必查）

```text
scheduler (RefCell)  ←→  ProcessRegistry (RefCell)     [spawn/fork 顺序窗口]
page-cache (Mutex×3+RwLock)  →  SharedRwFs (Mutex)      [驱逐写回]
EXT4_SMALL_READ_CACHE  ↔  BLOCK_DEVICES / CachingBlockDevice  [AB-BA 风险]
unix_sock BOUND  →  VFS mknod/metadata                    [已修复：VFS 先于 BOUND]
procfs LOOKUP  →  VFS/cwd 回调                            [已修复：锁外回调]
NETWORK_STACK  →  VirtIO / smoltcp                        [长临界区自旋]
```

---

## 三、Subagent 并行分组（15 路）

| # | 分组 ID | 覆盖结构 # | 输出文档 |
|---|---------|-----------|---------|
| 1 | `scheduler` | 3, 4 | `docs/audits/locks/scheduler.md` |
| 2 | `process-registry` | 2 | `docs/audits/locks/process-registry.md` |
| 3 | `per-task-registries` | 5, 6, 7 | `docs/audits/locks/per-task-registries.md` |
| 4 | `mm-allocators` | 8, 9 | `docs/audits/locks/mm-allocators.md` |
| 5 | `mount-rootfs` | 10, 11 | `docs/audits/locks/mount-rootfs.md` |
| 6 | `page-cache` | 12 | `docs/audits/locks/page-cache.md` |
| 7 | `shared-fs-handles` | 13 | `docs/audits/locks/shared-fs-handles.md` |
| 8 | `ipc-futex-signal-shm` | 14, 15, 16 | `docs/audits/locks/ipc-futex-signal-shm.md` |
| 9 | `ipc-pipe` | 17 | `docs/audits/locks/ipc-pipe.md` |
| 10 | `fs-aux` | 18–21 | `docs/audits/locks/fs-aux.md` |
| 11 | `driver-block-char` | 22–24 | `docs/audits/locks/driver-block-char.md` |
| 12 | `driver-network` | 25–27 | `docs/audits/locks/driver-network.md` |
| 13 | `platform-probe` | 28–30 | `docs/audits/locks/platform-probe.md` |
| 14 | `syscall-globals` | 31–34 | `docs/audits/locks/syscall-globals.md` |
| 15 | `klog` | 35 | `docs/audits/locks/klog.md` |
