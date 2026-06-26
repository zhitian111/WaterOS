# 资源审计修复任务队列

> 生成时间：2026-06-25  
> 来源：合并各 subagent 修复草案，按 P0→P1→P2 排序  
> 问题详情：[`resource-issues.md`](resource-issues.md)

---

## P0 — 立即修复（泄漏 / UAF / 卡死）

| 优先级 | 任务 ID | 标题 | 主要文件 | 验收标准 |
|--------|---------|------|---------|---------|
| P0 | T-PF-01 | fork 失败释放子地址空间 | `sys/clone.rs` | `fork_user_aspace` 后任何失败调用 `drop_user_aspace`；帧池无单调泄漏 |
| P0 | T-PF-02 | MAP_SHARED 帧引用计数闭环 | `pagetable.rs`, `address_space.rs` | fork 后单侧 munmap 不 UAF；双进程 exit 后帧回收 |
| P0 | T-PF-03 | 进程销毁回收共享匿名页 | `pagetable.rs::destroy_table` | MAP_SHARED 匿名 exit 后 PPN 回池 |
| P0 | T-KH-01 | 内核堆 OOM 可恢复路径 | `runtime-heap-allocator`, `main.rs` | 关键路径返回 `ENOMEM` 而非 panic（至少 spawn/fork/mmap 元数据） |
| P0 | T-KH-02 | KernelStack OOM 安全检查 | `task-api/.../kernel.rs` | `alloc_zeroed` 失败传播 `ENOMEM`，无 UB |
| P0 | T-KH-03 | recvfrom 内核缓冲上限 | `sys/recvfrom.rs` | 与 sendto 对齐 64KiB 上限 |
| P0 | T-TS-01 | clone fork 全路径失败回滚 | `sys/clone.rs`, `task/lib.rs` | `on_fork` 失败无孤儿 TCB/PID/就绪项 |
| P0 | T-TS-02 | clone 线程失败回滚 | `sys/clone.rs` | `on_clone_thread` 失败无多余 tid/TCB |
| P0 | T-FD-01 | openat partial alloc 回滚 | `sys/openat.rs` | flags/path_only 失败时 close 已分配 fd |
| P0 | T-FD-02 | pipe2 EFAULT 回滚 | `sys/pipe2.rs` | copy_to_user 失败关闭 read/write fd |
| P0 | T-FD-03 | PagedFileHandle close 释放 open_ref | `paged_handle.rs` | sync_dirty 失败仍 release_open_ref 或 defer 到 flush 重试 |
| P0 | T-PC-01 | unlink 前 flush 脏页 | `impl-page-cache`, `impl-fs-bridge` | unlink 已写数据不丢失 |
| P0 | T-PC-02 | rename 失效/迁移页缓存 | `impl-fs-bridge` | rename 后读写命中正确后端 |
| P0 | T-PC-03 | remount 前 flush_all | `mount_table.rs` | 代次切换无脏页丢弃 |
| P0 | T-PIPE-01 | 内部 drop_task_fd_table 须 close 句柄 | `registry.rs` | 与 `vfs::fd::drop_task_fd_table` 语义一致 |
| P0 | T-IPC-01 | 信号等待临时 WaitQueue 释放 | `sys/signal.rs`, `wait_queue.rs` | sigtimedwait 1e4 次后 wait_queues 不线性增长 |
| P0 | T-IPC-02 | shm fork 失败回滚 nattch | `ipc-shm/lib.rs` | MM 失败回滚 registry 与 nattch |
| P0 | T-IPC-03 | futex WAIT/WAKE key 对称 | `sys/futex.rs`, `hub.rs` | private/shared 混用不永久睡眠 |
| P0 | T-SKT-01 | unix BOUND 引用计数 | `unix_sock.rs` | 子进程 close 继承 listen fd 不删全局绑定 |
| P0 | T-SKT-02 | execve CLOEXEC 清理 socket 侧表 | `sys/execve.rs`, `socket_fd.rs`, `unix_sock.rs` | exec 后 fd 复用不走陈旧 SocketRef |
| P0 | T-FS-01 | umount2 busy 检查 | `mount_table.rs`, `sys/umount2.rs` | 有 open fd 时返回 `-EBUSY` |
| P0 | T-FS-02 | 同设备 RW 挂载去重或拒绝 | `impl-ext4-rs`, `mount_table.rs` | 双实例不写同一块设备 |
| P0 | T-FS-03 | device_minor 耗尽返回错误 | `mount_table.rs` | 不 panic，返回 `-ENOSPC` 或内部 Err |

---

## P1 — 错误路径 / 慢泄漏 / 限额

| 优先级 | 任务 ID | 标题 | 主要文件 | 验收标准 |
|--------|---------|------|---------|---------|
| P1 | T-PF-04 | brk 扩页失败返回 ENOMEM | `sys/brk.rs` | OOM 时 break 不变 |
| P1 | T-PF-05 | mmap/brk partial alloc 回滚 | `common/lib.rs`, `user_heap_mmap.rs` | 失败区间无残留 PTE |
| P1 | T-PF-06 | munmap 回收空闲页表中间帧 | `pagetable.rs` | 10⁴ 次 mmap/munmap 帧不线性增长 |
| P1 | T-TS-03 | 接入 RLIMIT_NPROC | `process.rs`, `lib.rs` | 超限创建返回错误 |
| P1 | T-PIPE-02 | Pipe Drop 回收 WaitQueueId | `kernel_pipe.rs` | 1000 pipe 后 ID 可复用 |
| P1 | T-SKT-03 | socket() 失败回滚 smoltcp | `sys/socket.rs` | alloc_fd 失败不泄漏 SocketSet 项 |
| P1 | T-SKT-04 | dup/fcntl 同步 unix FD_TABLE | `dup.rs`, `fcntl.rs` | dup unix fd 后 getsockname 成功 |
| P1 | T-SKT-05 | smoltcp socket 全局上限 | `driver-network/stack` | 超限 `-ENOMEM` + warn |
| P1 | T-FS-04 | tmpfs 堆用量上限或 warn | `tmpfs.rs` | 大 tmpfs 写入有界或明确失败 |
| P1 | T-DRV-01 | init_after_boot 幂等或清空设备表 | `impl-qemu-*` | 重复 init 不追加设备 |
| P1 | T-DRV-02 | DMA/帧池高水位 warn | `impl-virtio-*`, `impl-stack` | 90% 用量打印 warn |

---

## P2 — 文档 / 可观测性 / 中期优化

| 优先级 | 任务 ID | 标题 | 验收标准 |
|--------|---------|------|---------|
| P2 | T-PF-07 | 帧池高水位 warn | free < 10% 时 warn |
| P2 | T-DOC-01 | 更新 wateros-runtime.md 分配器描述 | 与 linked_list 实码一致 |
| P2 | T-DOC-02 | features 组件注明资源限额 | 指向本审计文档 |

---

## 建议实施顺序

1. **第一波（同源修复）**：T-PF-01 + T-TS-01/02 + T-KH-02 — **已完成（2026-06-25）**
2. **第二波（fd 账本）**：T-FD-01/02/03 + T-PIPE-01 — **T-FD-01/02/03、T-PIPE-01 已完成**；T-SKT-02 待做
3. **第三波（内存安全）**：T-PF-02/03 + T-IPC-02
4. **第四波（卡死）**：T-PIPE-01 + T-IPC-01/03 + T-SKT-01
5. **第五波（FS/缓存）**：T-PC-* + T-FS-* — **已完成（2026-06-25）**
6. **第六波（内核堆 OOM）**：T-IPC-01 + T-KH-01（局部）+ KH-P1-2/6 — **已完成（2026-06-26）**
   - `rt_sigsuspend`/`rt_sigtimedwait`/`ltp_cgroup_helper`：`WaitQueue::try_release_empty`
   - `fallible_buf` + `getdents64`/`read`/`write`/`sockopt`/`sched` 可失败分配
   - `paged_handle` detached 16MiB 上限
   - `heap_mem_stats()` + 90% 高水位 warn
   - `unix_sock` accept/dgram 队列上限

---

## 验证方式

```bash
cd os && make rv_check          # 编译
cd os && make rv_qemu_run       # 运行时回归
# LTP 分阶段：user_bringup_busybox.rs SCRIPT_PATHS
```

---

## 回填

- 高优先级项建议同步 [`docs/roadmap/todolist.md`](../roadmap/todolist.md)
- 已收敛路径在 [`resource-issues.md`](resource-issues.md) 标注「已收敛 / 待实现」
- 与 syscall 审计重复项只修一处，以本队列与 `syscall-issues.md` 交叉引用为准
