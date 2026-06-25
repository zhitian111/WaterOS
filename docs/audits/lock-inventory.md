# 带锁数据结构清单

> 生成时间：2026-06-25  
> 搜索范围：`os/components/**`（含平台驱动 probe 静态量）  
> Baseline：单核多线程；`UniprocessorSafeCell` 视为单核独占原语，`spin::Mutex` / `spin::RwLock` 为自旋锁。

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
| 17 | `KernelPipe` / `PipeState` | `wateros-ipc/ipc-pipe/pipe-impl/impl-ringbuf/src/kernel_pipe.rs` | `UniprocessorSafeCell` | 中 | `ipc-pipe` |
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

## 未纳入（原子变量，非显式锁）

| 名称 | 文件 | 说明 |
|------|------|------|
| `REALTIME_OFFSET_NS` | `wateros-platform/src/wall_clock.rs` | `AtomicI64`，Relaxed 序；与 `TIMEX_STATE` 审计交叉关注 |

## Subagent 并行分组（15 路）

1. `scheduler` — #3, #4  
2. `process-registry` — #2  
3. `per-task-registries` — #5, #6, #7  
4. `mm-allocators` — #8, #9  
5. `mount-rootfs` — #10, #11  
6. `page-cache` — #12  
7. `shared-fs-handles` — #13  
8. `ipc-futex-signal-shm` — #14, #15, #16  
9. `ipc-pipe` — #17  
10. `fs-aux` — #18–#21  
11. `driver-block-char` — #22–#24  
12. `driver-network` — #25–#27  
13. `platform-probe` — #28–#30  
14. `syscall-globals` — #31–#34  
15. `klog` — #35  
