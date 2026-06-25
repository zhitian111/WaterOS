# 可分配资源清单

> 生成时间：2026-06-25  
> 搜索范围：`os/components/**`（含 syscall 入口到子系统实现的跨模块调用链）  
> Baseline：单核多线程；对照 Linux 常见资源语义（`ENOMEM`、`EMFILE`、`EBADF` 等）  
> 交叉参考：`docs/audits/lock-inventory.md`、`docs/audits/syscall-issues.md`

## 资源总览

| # | 资源名称 | 所属组件 | 主要类型/结构体 | 分配 API（入口） | 回收 API（入口） | 硬上限 | 复杂度 | Subagent |
|---|---------|---------|----------------|-----------------|-----------------|--------|--------|----------|
| 1 | 物理页帧 | `wateros-mm` / `impl-stack` | `StackFrameAllocator`、`PhysPageNum` | `frame_alloc()` / `alloc_frame()` | `frame_dealloc()` / `dealloc_frame()` | 由 `init_frame_allocator(start,end)` 限定 | 中 | `physical-frames` |
| 2 | 页表帧（中间/叶子） | `wateros-mm` / `impl-sv39` 等 | `PhysPageNum`（与数据帧共用池） | `alloc_table_frame_zeroed()` | `destroy_table()` → `frame_dealloc` | 受帧池约束 | 中 | `physical-frames` |
| 3 | 用户虚拟页映射 | `wateros-mm` | `Sv39AddressSpace`、VMA 元数据 | `brk()` / `mmap()` / `handle_page_fault()` | `munmap()` / `destroy_table()` | VA `0x4000_0000_0000`；栈 256KiB 保留 | 高 | `physical-frames` |
| 4 | 用户地址空间对象 | `wateros-mm` | `Sv39AddressSpace`（`Box::leak`） | `from_elf_path()` / `fork_user_aspace()` | `drop_user_aspace()` / `drop_user_aspace_on_task_exit()` | ASID 65535 循环复用 | 高 | `physical-frames` |
| 5 | 内核全局页表 | `wateros-mm` | `Sv39AddressSpace` | `kernel_mm::init()` + `Box::leak` | **无**（故意 leak） | 引导期一次性 | 中 | `physical-frames` |
| 6 | 内核堆 | `wateros-runtime` / `runtime-heap-allocator` | `InterruptSafeLockedHeap` | `GlobalAlloc::alloc()` | `GlobalAlloc::dealloc()` | **128 MiB** | 低 | `kernel-heap` |
| 7 | 任务槽位 / TaskId | `wateros-task` / `scheduler-api` | `TaskTable`、`TaskSlot` | `TaskTable::allocate_id()` | `TaskTable::remove()` → `reap_task()` | slot 索引 u32；受堆 Vec 限制 | 中 | `task-slots` |
| 8 | 任务控制块 TCB | `wateros-task` / `impl-core` | `TaskControlBlock` | `new_kernel_task` / `new_user_task` / `fork_from` | `reap_task()` → Drop | 与任务槽同阶 | 中 | `task-slots` |
| 9 | 内核栈 | `wateros-task` / `task-api` | `KernelStack`（32 KiB） | `KernelStack::new()` | TCB Drop | **32 KiB/任务** | 低 | `task-slots` |
| 10 | 进程槽位 PID | `wateros-task` / `impl-core` | `ProcessRegistry`、`ProcessControlBlock` | `alloc_pid()` / `create_process_for_task()` | `reap_process()` / `reap_process_with_tasks()` | 无显式上限（usize 递增） | 中 | `task-slots` |
| 11 | 线程 TID / 进程内线程列表 | `wateros-task` | `ProcessTask`、`Vec<ProcessTask>` | `alloc_tid()` / `add_task_to_process()` | 进程 reap / `retain_only_task` | 无显式每进程线程数上限 | 中 | `task-slots` |
| 12 | 调度 WaitQueueId | `wateros-task` / `scheduler-api` | `WaitQueues` | `allocate_wait_queue()` | `try_release_wait_queue()` | 无硬上限，id 可复用 | 低 | `task-slots` |
| 13 | Per-task FD 槽位 | `wateros-vfs` / `impl-fd-session` | `PerTaskFdRegistry` | `alloc_fd()` / `alloc_fd_for_task()` | `close_fd()` / `drop_task_fd_table()` | **RLIMIT_NOFILE=1024** 默认 | 高 | `file-descriptors` |
| 14 | FD 标志位 / owner / ref_count | `impl-fd-session` | `BTreeMap<TaskId, Vec<u8>>` 等 | `ensure_flags_len` / `share_fd_table_from_parent` | `close_slot` / `release_owner` | 与 fd 同阶 | 中 | `file-descriptors` |
| 15 | VfsIoHandle（fd 句柄） | `wateros-vfs` | `Box<dyn VfsIoHandle>` | `FsBridge::open_path` + `alloc_fd*` | `VfsIoHandle::close` / fd close | 受 rlimit | 高 | `file-descriptors` |
| 16 | PerTaskCwdRegistry | `wateros-vfs` | cwd/exe/argv 表 | `init_task_cwd` / `set_task_cwd` 等 | `drop_task` | PATH_MAX=256 | 中 | `file-descriptors` |
| 17 | 页缓存帧（全局 LRU） | `impl-page-cache` | `GlobalCacheState`、`PageFrame` | `install_page` / `install_zero_page` | LRU 驱逐 / `purge_closed_file` | **4096 帧 × 4KiB ≈ 16MiB** | 高 | `page-cache` |
| 18 | 页缓存 per-file 元数据 | `impl-page-cache` | `FileEntryInner`、`open_refs` | `get_file_entry` / `acquire_open_ref` | `release_open_ref` / `purge_closed_file` | 无硬上限（靠 close purge） | 中 | `page-cache` |
| 19 | 块设备缓存槽 | `driver-block` / `impl-block-cache` | `CachingBlockDevice::slots` | `CachingBlockDevice::new`（预分配） | LRU `evict_lru_slot` | **64 块** 默认 | 中 | `block-cache` |
| 20 | Pipe 对象 + 端点 | `wateros-ipc` / `impl-ringbuf` | `Pipe`、`PipeEndpoint` | `Pipe::with_capacity()` / `pipe_handle_pair()` | `PipeEndpoint::close()` / fd close | 缓冲默认 **4096B/pipe** | 中 | `pipe-buffers` |
| 21 | Pipe 环形缓冲 | `impl-ringbuf` | `PipeState.buf: Vec<u8>` | `PipeState::with_capacity` | 随 `Arc<Pipe>` Drop | 固定容量/pipe | 低 | `pipe-buffers` |
| 22 | Futex 等待队列 | `wateros-ipc` / `ipc-futex` | `FutexHub`、`FutexTables` | `get_queue()` 惰性插入 | `cleanup_empty_queue()` | 无显式上限 | 高 | `ipc-shm-futex-signal` |
| 23 | Futex robust 侧表 | `ipc-futex` | `robust: BTreeMap<TaskId, RobustState>` | `set_robust_list()` | `drop_robust_list()` / `robust_exit_cleanup` | 每 task 1 条；遍历限 4096 步 | 高 | `ipc-shm-futex-signal` |
| 24 | 进程/线程信号状态 | `wateros-ipc` / `ipc-signal` | `SignalRegistry` | `register_process()` / `register_thread()` | `drop_process()` / `drop_thread()` | NSIG=64 | 高 | `ipc-shm-futex-signal` |
| 25 | SysV SHM 段 | `wateros-ipc` / `ipc-shm` | `ShmSegment`、`ShmRegistry` | `create_or_get()` / `sys_shmget` | `remove_segment()` / `IPC_RMID` | 单段最大 **4 MiB** | 高 | `ipc-shm-futex-signal` |
| 26 | SHM 附着记录 | `ipc-shm` | `ShmAttachment` | `begin_attach()` / `sys_shmat` | `detach()` / `drop_task_attachments` | 无专用 cap | 中高 | `ipc-shm-futex-signal` |
| 27 | Inet socket（smoltcp） | `wateros-driver` / `driver-network` | `SocketHandle`、`NetworkStack` | `create_tcp_socket()` / `create_udp_socket()` | `socket_close()` | 无显式数量上限；TCP 约 512KiB/socket | 高 | `sockets` |
| 28 | SocketFdRegistry | `wateros-syscall` | `socket_fd.rs` | `register_with_flags()` | `remove()` / `drop_task()` | 受 RLIMIT_NOFILE | 中高 | `sockets` |
| 29 | Unix 域套接字 | `wateros-syscall` | `unix_sock.rs`：`FD_TABLE`、`BOUND` | `alloc_unix_socket()` / `bind()` | `unregister()` / `drop_task()` | fd 受 rlimit；队列无界 VecDeque | 高 | `sockets` |
| 30 | 根卷 RO/RW 全局槽 | `wateros-fs` / `rootfs` | `ROOT_FS` / `ROOT_RW_FS` | `mount_root_*_from_block_path` | `clear_root_fs` | 各 **1** 槽 | 低 | `fs-instances` |
| 31 | 辅助挂载表项 | `wateros-vfs` / `mount_table` | `AUX_MOUNTS: Vec<MountEntry>` | `mount_aux_*` / `mount_tmpfs_at` | `unmount_aux_at` | device_minor 至 u32::MAX panic | 中 | `fs-instances` |
| 32 | SharedFs / SharedRwFs 实例 | `wateros-fs` + `impl-fs-bridge` | `Arc<Mutex<LocalFs>>` | `FsImpl::mount_ro/rw` | unmount 丢弃 Arc | 根 1 + 辅助卷无硬上限 | 中 | `fs-instances` |
| 33 | ext4 inode（磁盘） | `impl-ext4-rs` | `Ext4` + inode_ref | `create_regular` / `create_directory` | `unlink` → `ialloc_free_inode` | 卷级位图 | 高 | `fs-instances` |
| 34 | TmpFs inode / 节点树 | `impl-fs-bridge` / `tmpfs` | `TmpFs`、`TmpNode` | `alloc_inode` / `mkdir` 等 | `unlink` / `rmdir`（**inode 不回收**） | 仅受堆约束 | 中 | `fs-instances` |
| 35 | DevFS 节点 | `devfs-impl-kernel` | `DevFsImpl` | `refresh` / `register_*_device` | `refresh` 全量重建 | 设备数 | 中 | `fs-instances` |
| 36 | 块设备注册槽 | `driver-block` | `BLOCK_DEVICES: Vec` | `register_block_device()` | **无 unregister** | 无代码上限 | 低 | `driver-slots` |
| 37 | 字符设备注册槽 | `driver-character` | `CHARACTER_DEVICES: Vec` | `register_character_device()` | **无 unregister** | 无代码上限 | 低 | `driver-slots` |
| 38 | 网络设备注册槽 | `driver-network` | `NETWORK_DEVICES: Vec` | `register_network_device()` | **无 unregister** | 无代码上限 | 低 | `driver-slots` |
| 39 | VirtIO DMA 物理页 | `impl-virtio-*` | HAL `dma_alloc` | `dma_alloc()` | `dma_dealloc()` | 受帧池 | 中 | `driver-slots` |
| 40 | PCI MMIO BAR 地址 | `impl-virtio-pci` | `VirtioPciBarAllocator` | `BarAllocator::allocate` | **无回收**（bump） | 窗口 0x4000_0000..0x8000_0000 等 | 中 | `driver-slots` |
| 41 | Klog 环形缓冲 | `wateros-klog` | `KlogRingbufInner` | 写日志入队 | 读/覆盖（若实现） | 取决于 ringbuf 容量 | 中 | `driver-slots`（附带） |

## 跨资源生命周期钩子（汇总）

| 事件 | 涉及资源回收 | 主要入口 |
|------|-------------|---------|
| `close(fd)` | fd 槽、VfsIoHandle、pipe、socket_fd、unix_sock | `sys/close.rs` |
| 线程/任务退出 | fd 表、cwd、shm attach、socket、user aspace | `sys/task.rs` → `drop_task_runtime_resources_with_aspace` |
| 进程 reap | user aspace、进程信号、PID 槽 | `ProcessRegistry::reap_process_with_tasks` |
| `fork` | fd 表共享、signal 复制、socket_fd/unix_sock 复制、shm fork | `sys/clone.rs` |
| `execve` | shm detach、robust 清理、signal exec | `sys/execve.rs` |
| `munmap` / brk 收缩 | 用户页、VMA 元数据 | `wateros-mm` |
| 辅助卸载 | SharedFs Arc、挂载表项 | `unmount_aux_at` |

## Subagent 并行分组（11 路）

| 分组 ID | 覆盖资源 # | 输出文件 |
|---------|-------------|---------|
| `physical-frames` | 1–5 | `docs/audits/resources/physical-frames.md` |
| `kernel-heap` | 6 | `docs/audits/resources/kernel-heap.md` |
| `task-slots` | 7–12 | `docs/audits/resources/task-slots.md` |
| `file-descriptors` | 13–16 | `docs/audits/resources/file-descriptors.md` |
| `page-cache` | 17–18 | `docs/audits/resources/page-cache.md` |
| `block-cache` | 19 | `docs/audits/resources/block-cache.md` |
| `pipe-buffers` | 20–21 | `docs/audits/resources/pipe-buffers.md` |
| `ipc-shm-futex-signal` | 22–26 | `docs/audits/resources/ipc-shm-futex-signal.md` |
| `sockets` | 27–29 | `docs/audits/resources/sockets.md` |
| `fs-instances` | 30–35 | `docs/audits/resources/fs-instances.md` |
| `driver-slots` | 36–41 | `docs/audits/resources/driver-slots.md` |

## 初步风险热点（待 subagent 验证）

1. **高复杂度路径**：fork COW、mmap 懒加载、fd 表 fork/dup、页缓存 LRU 写回、futex requeue、unix_sock 多表
2. **无显式上限**：futex 队列数、unix_sock accept/dgram 队列、smoltcp socket 数、tmpfs inode 单调递增
3. **无注销 API**：三类设备注册表、PCI BAR bump、内核全局页表故意 leak
4. **ID 不回收**：ASID、mount_id/device_minor、tmpfs/pipe 伪 inode
5. **大内存风险**：`BufferedFileHandle` 整文件读入堆；与页缓存 16MiB 上限无关
