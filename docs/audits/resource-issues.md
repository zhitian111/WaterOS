# 资源生命周期潜在问题清单

> 生成时间：2026-06-25  
> 来源：11 路 subagent 单资源审计（`docs/audits/resources/*.md`）去重合并  
> Baseline：单核多线程；对照 Linux `ENOMEM`/`EMFILE`/`EBADF` 等  
> 交叉参考：[`syscall-issues.md`](syscall-issues.md)、[`lock-inventory.md`](lock-inventory.md)

---

## 严重程度说明

| 级别 | 含义 |
|------|------|
| **P0** | 泄漏、UAF、卡死、或可导致全局耗尽后雪崩 |
| **P1** | 错误路径未回滚、错误码不符、慢泄漏、静默耗尽 |
| **P2** | 设计已知、文档偏差、长期可接受但需标注 |

---

## P0 — 泄漏 / UAF / 卡死

### 内存与地址空间（`physical-frames` / `kernel-heap` / `task-slots`）

| ID | 类型 | 描述 | 位置 | 分组 |
|----|------|------|------|------|
| PF-P0-01 | 泄漏 | `fork_user_aspace` 成功后，`fork_current`/`on_fork` 失败未 `drop_user_aspace` | `sys/clone.rs` | physical-frames |
| PF-P0-02 | UAF | `MAP_SHARED` 映射 fork 不 `inc_ref`，单侧 `munmap` 直接 `dealloc_frame` | `pagetable.rs` | physical-frames |
| PF-P0-03 | 泄漏 | `destroy_table` 跳过 `shared_anon_vmas` 页释放 | `pagetable.rs` | physical-frames |
| KH-P0-1 | 崩溃 | 堆 OOM 经 `alloc_error_handler` 全局 panic，无恢复路径 | `runtime-heap-allocator` | kernel-heap |
| KH-P0-2 | UB | `KernelStack::new` 未检查 `alloc_zeroed` 失败 | `task-api/.../kernel.rs` | kernel-heap / task-slots |
| KH-P0-3 | 崩溃 | `sys_recvfrom` 按用户 `len` 无上限分配内核缓冲 | `sys/recvfrom.rs` | kernel-heap |
| TS-P0-1 | 泄漏+孤儿 | `fork_current` 成功后 `on_fork` 失败无回滚，子任务仍可调度 | `sys/clone.rs` | task-slots |
| TS-P0-2 | 泄漏 | clone 线程 `on_clone_thread` 失败无回滚 | `sys/clone.rs` | task-slots |
| TS-P0-4 | 卡死 | `Exited` 须先 `wake_all_waiters_for_task_exit` 再 `detach`（需回归） | `wait_queues.rs` | task-slots |

### 文件描述符与页缓存（`file-descriptors` / `page-cache`）

| ID | 类型 | 描述 | 位置 | 分组 |
|----|------|------|------|------|
| FD-P0-01 | 泄漏 | `openat`：`alloc_fd` 后 `set_fd_flags`/`set_path_only_fd` 失败未 close 回滚 | `sys/openat.rs` | file-descriptors |
| FD-P0-02 | 泄漏 | `pipe2`：`copy_to_user` `-EFAULT` 时已分配 read/write fd 未关闭 | `sys/pipe2.rs` | file-descriptors |
| FD-P0-03 | 泄漏 | `PagedFileHandle::close`：`sync_dirty` 失败跳过 `release_open_ref` | `paged_handle.rs` | file-descriptors / page-cache |
| PC-LC-01 | 数据丢失 | `unlink_path` → `purge_closed_file` 不 flush 即回收脏页 | `impl-page-cache` | page-cache |
| PC-LC-02 | 泄漏 | 同 FD-P0-03：`open_refs`/`files` 永久泄漏 | 同上 | page-cache |
| PC-LC-03 | 语义错误 | `rename_path` 不迁移/失效页缓存 | `impl-fs-bridge` | page-cache |
| PC-LC-04 | 数据丢失 | 挂载别名复用根卷时 `bump_mount_generation` 未 `flush_all` | `mount_table.rs` | page-cache / fs-instances |
| PC-LC-05 | 账本错乱 | remount 后 `global_cache` 代次与句柄键错位 | `impl-page-cache` | page-cache |

### IPC（`pipe-buffers` / `ipc-shm-futex-signal`）

| ID | 类型 | 描述 | 位置 | 分组 |
|----|------|------|------|------|
| PIPE-P0-01 | 卡死 | `registry.drop_task_fd_table` 无声丢弃句柄，pipe 端点引用不更新 | `registry.rs` | pipe-buffers |
| PIPE-P0-02 | 卡死 | pipe 销毁未唤醒阻塞者（PIPE-P0-01 加重） | `kernel_pipe.rs` | pipe-buffers |
| SIG-P0-01 | 耗尽 | `rt_sigsuspend`/`rt_sigtimedwait` 每次 `WaitQueue::new` 不释放 | `sys/signal.rs` | ipc-shm-futex-signal |
| ATT-P0-01 | 泄漏/UAF | `fork_task_attachments` MM 失败不回滚 `nattch` | `ipc-shm` | ipc-shm-futex-signal |
| FUT-P0-02 | 卡死 | Futex WAIT/WAKE private/shared key 不对称 | `sys/futex.rs` | ipc-shm-futex-signal |

### 套接字（`sockets`）

| ID | 类型 | 描述 | 位置 | 分组 |
|----|------|------|------|------|
| SKT-29-P0-01 | 全局破坏 | 子进程 close 继承 listening socket 删除 `BOUND` 表项 | `unix_sock.rs` | sockets |
| SKT-28-P0-01 | UAF语义 | `execve` CLOEXEC 未 `socket_fd::remove`，fd 复用后误路由 | `sys/execve.rs` | sockets |
| SKT-29-P0-02 | UAF语义 | `execve` CLOEXEC 未 `unix_sock::unregister` | `unix_sock.rs` | sockets |

### 文件系统（`fs-instances`）

| ID | 类型 | 描述 | 位置 | 分组 |
|----|------|------|------|------|
| FI-01 | 数据损坏 | 同块设备多次独立 RW 挂载无去重 | `impl-ext4-rs` | fs-instances |
| FI-02 | UAF语义 | `umount2` 不检查 busy（打开 fd / open_refs） | `mount_table.rs` | fs-instances |
| FI-03 | 崩溃 | `device_minor` 耗尽时 panic | `mount_table.rs` | fs-instances |

### 驱动（`driver-slots`）

| ID | 类型 | 描述 | 位置 | 分组 |
|----|------|------|------|------|
| D1 | 泄漏 | 设备注册表无幂等；`init_after_boot` 重复调用追加设备 | `driver-api` | driver-slots |
| D2 | 静默耗尽 | VirtIO DMA 帧永久占用，无配额/预警 | `impl-virtio-*` | driver-slots |

---

## P1 — 错误路径 / 慢泄漏 / 语义偏差

| ID | 描述 | 分组 |
|----|------|------|
| PF-P1-01 | `munmap`/`brk` 不回收中间页表帧 | physical-frames |
| PF-P1-02 | `brk` 扩页 OOM 返回旧 break 而非 `-ENOMEM` | physical-frames |
| PF-P1-03 | `map_zeroed_range` / brk 扩页无 partial alloc 回滚 | physical-frames |
| PF-P1-04 | `frame_dealloc()` 忽略 `InvalidFrame` | physical-frames |
| TS-P1-1 | `RLIMIT_NPROC` 未接入创建路径 | task-slots |
| TS-P1-2 | 临时 `WaitQueue` 未 `try_release_empty`（与 SIG-P0-01 重叠） | task-slots |
| PIPE-P1-01 | 每 pipe 2 个 `WaitQueueId` 销毁不释放 | pipe-buffers |
| SKT-29-P1-01 | unix `dup` 未注册 `FD_TABLE` | sockets |
| SKT-29-P1-03 | accept/dgram 队列无界 `VecDeque` | sockets |
| FI-tmpfs | tmpfs 无堆上限，inode 不回收 | fs-instances |
| BC-P1-01 | block cache LRU 不变量破坏时 `expect` panic | block-cache |

---

## P2 — 设计已知 / 文档偏差

| ID | 描述 |
|----|------|
| PF-P2-01 | 内核全局页表故意 `Box::leak` |
| PF-P2-04 | 无 `RLIMIT_AS` 类用户内存硬顶 |
| PIPE-P2-02 | pipe 伪 inode 单调递增不复用 |
| 文档漂移 | `wateros-runtime.md` 写 buddy 分配器，实码为 linked_list |

---

## 账本稳定性总评（按分组）

| 分组 | 结论 | 主要风险 |
|------|------|---------|
| physical-frames | **部分稳定** | MAP_SHARED UAF、fork 失败泄漏 |
| kernel-heap | **部分稳定** | syscall 热路径 OOM 已局部可恢复；引导期仍 panic |
| task-slots | **部分稳定** | fork/clone 错误路径无回滚 |
| file-descriptors | **部分稳定** | partial alloc、openat/pipe2 回滚 |
| page-cache | #17 稳定 / #18 **不可靠** | sync 失败泄漏、rename/remount |
| block-cache | **稳定** | 无 P0 |
| pipe-buffers | **部分稳定** | 内部 fd 表重置路径 |
| ipc-shm-futex-signal | **部分稳定** | shm fork；futex bitset |
| sockets | **部分稳定** | unix 队列已加上限；BOUND/execve 已修 |
| fs-instances | **部分稳定** | busy umount、双 RW 挂载 |
| driver-slots | **部分稳定**（启动期） | DMA 常驻、无注销 |

---

## 已收敛（2026-06-26 内核堆 OOM 波次）

| ID | 状态 | 说明 |
|----|------|------|
| SIG-P0-01 / T-IPC-01 | **已收敛** | `sys/signal.rs`、`sys/task.rs`、`ltp_cgroup_helper.rs` 释放临时 `WaitQueue` |
| KH-P0-1 / T-KH-01 | **部分收敛** | syscall/VFS 热路径 `try_kbuf`/cap 返回 `ENOMEM`；引导期 `alloc_error_handler` 仍 panic |
| KH-P0-2 / T-KH-02 | **已收敛** | `KernelStack::try_new` |
| KH-P0-3 / T-KH-03 | **已收敛** | `recvfrom` 64KiB 上限 |
| KH-P1-2 | **已收敛** | `unix_sock` accept/dgram 队列上限 |
| KH-P1-5 | **已收敛** | `base-config/fs.rs` 堆大小注释 128MiB |
| KH-P1-6 | **已收敛** | `heap_mem_stats()` + 90% warn |
| PF-P0-01 / KH-P0-4 | **已收敛** | `clone` fork 失败 `drop_user_aspace` |

---

## 与 syscall / 锁审计交叉项

| 交叉项 | 说明 |
|--------|------|
| `syscall-issues` P0-04/05 | mmap 族无 aspace 已收敛；本审计关注有 aspace 后帧账本 |
| `syscall-issues` P0-08/09 | futex bitset/private 与本审计 FUT-P0-02 重叠 |
| `lock-inventory` #8 | `StackFrameAllocator` 禁止嵌套 `frame_alloc` |
| `lock-inventory` #12 | 页缓存三锁 + per-file RwLock，与 PC-LC 问题相关 |
| FD + unix_sock + socket_fd | 同一 fd 路径三处侧表，execve/fork/close 须同步（见 sockets、file-descriptors） |

---

## 单资源详细说明

完整生命周期、状态机、代码锚点见：

- [`docs/audits/resources/`](resources/) 目录下 11 份分组文档
- [`docs/audits/resource-inventory.md`](resource-inventory.md) 可分配资源总表
