# Pipe 缓冲区资源生命周期审计

> **分组 ID**：`pipe-buffers`  
> **覆盖资源**：#20 Pipe 对象 + PipeEndpoint、#21 Pipe 环形缓冲  
> **审计时间**：2026-06-25  
> **Baseline**：单核多线程；对照 Linux `pipe2`/`read`/`write`/`close`/`dup`/`fork` 语义  
> **交叉参考**：`docs/audits/resource-inventory.md` §20–21、`docs/audits/syscall-issues.md` IO-P1-03/06、`docs/audits/lock-inventory.md`

---

## 1. 资源概览

| # | 资源名称 | 所属组件 | 主要类型 | 单次分配体量 | 硬上限 |
|---|---------|---------|---------|-------------|--------|
| 20 | Pipe 对象 + 端点 | `wateros-ipc/ipc-pipe` → `impl-ringbuf` | `Pipe`、`PipeEndpoint`、`PipeReadHandle`/`PipeWriteHandle` | 1×`Arc<Pipe>` + 2×端点 + 2×`WaitQueueId` | 无全局 pipe 数量上限 |
| 21 | 环形缓冲 | `impl-ringbuf` | `PipeState.buf: Vec<u8>` | **4096 B**（`DEFAULT_PIPE_CAPACITY`） | 每 pipe 固定容量；`capacity==0` 拒绝 |

**配置常量**：`os/components/wateros-base/base-config/src/ipc.rs` → `DEFAULT_PIPE_CAPACITY = 4096`。

**用户态可见路径**：`pipe2(2)` → `vfs::pipe_handle_pair` → `PipeEndpoint::pair`；`socketpair(AF_UNIX)` 额外消耗 **2 个 Pipe**（4 个端点）；`unix_sock` 内建连接亦经 `stream_pair_handle_pair`。

---

## 2. 分配入口

### 2.1 内核直接分配

| 函数 | 文件 | 说明 |
|------|------|------|
| `Pipe::with_capacity(n)` | `ipc-pipe/pipe-impl/impl-ringbuf/src/kernel_pipe.rs` | 分配 `Mutex<PipeState>`、`Vec<u8>` 容量 `n`、两个 `WaitQueue` |
| `Pipe::new()` | 同上 | `with_capacity(DEFAULT_PIPE_CAPACITY)` |
| `PipeEndpoint::pair(nonblocking)` | `ipc-pipe/pipe-impl/impl-ringbuf/src/endpoint.rs` | `Arc::new(Pipe::new())` + `acquire_read()` + `acquire_write()` 各 +1 |
| `pipe_handle_pair(nonblocking)` | `vfs-impl/impl-fd-session/src/handles.rs` | 包装为 `PipeReadHandle`/`PipeWriteHandle`，分配单调递增伪 inode |
| `stream_pair_handle_pair` | 同上 | 2×`PipeEndpoint::pair`，交叉接线为双向 Unix stream pair |

### 2.2 Syscall / 初始化路径

| 入口 | 文件 | 分配链 |
|------|------|--------|
| `sys_pipe2` | `syscall-impl/impl-kernel/src/sys/pipe2.rs` | `pipe_handle_pair` → `alloc_fd_for_task` ×2 |
| `sys_socketpair` | `syscall-impl/impl-kernel/src/sys/socketpair.rs` | `stream_pair_handle_pair` → `alloc_fd_for_task` ×2 |
| `unix_sock` bind/accept 内建 | `syscall-impl/impl-kernel/src/unix_sock.rs` | `stream_pair_handle_pair` |

### 2.3 分配前置依赖

- **内核堆**：`Vec<u8>`、`Arc`、`Box<dyn VfsIoHandle>` 均走全局堆（128 MiB，`runtime-heap-allocator`）。
- **FD 槽位**：受 per-task `RLIMIT_NOFILE`（默认 1024）约束；`pipe2` 一次占 2 槽。
- **WaitQueueId**：每 `Pipe` 分配 2 个（`read_wait`、`write_wait`），来自调度器 `allocate_wait_queue()`。
- **容量校验**：`PipeState::with_capacity(0)` → `PipeError::InvalidCapacity`；`pipe2` 始终用默认 4096，不经用户指定容量。

### 2.4 错误路径回滚（`pipe2`）

| 失败点 | 回滚 |
|--------|------|
| `alloc_fd_for_task` 写端失败 | `close_fd_for_task(read_fd)` ✓ |
| `O_CLOEXEC` 设置失败 | `close_fd` 两端 ✓ |
| `copy_to_user(pipefd)` 失败 | **不回滚**；fd 已入表（与 Linux 行为一致，见 §5.3） |

---

## 3. 回收入口

### 3.1 显式关闭

| 路径 | 文件 | 行为 |
|------|------|------|
| `sys_close` | `syscall-impl/impl-kernel/src/sys/close.rs` | `vfs::fd::close_fd` → `take_fd_for_close` → `VfsIoHandle::close()` |
| `PipeReadHandle::close` / `PipeWriteHandle::close` | `vfs-impl/impl-fd-session/src/handles.rs` | `PipeEndpoint::close()` → `release_read` / `release_write` |
| `PipeEndpoint::close` | `ipc-pipe/.../endpoint.rs` | 引用计数 -1；归零时 `read_open`/`write_open = false` 并 `wake_all` 对端等待队列 |
| `dup3` 覆盖目标 fd | `registry.rs` `dup3_fd_for_task` | 先 `close_slot` 旧句柄 ✓ |
| `execve` CLOEXEC | `vfs/src/fd.rs` `close_cloexec_fds_for_current_task` | `take_cloexec` + `handle.close()` ✓ |

### 3.2 任务 / 进程退出

| 事件 | 入口 | pipe 回收 |
|------|------|-----------|
| 线程 `exit` / `exit_group` | `sys/task.rs` → `drop_task_runtime_resources` → `vfs::fd::drop_task_fd_table` | `drain_task_fd_table` 取出全部句柄后 **逐个 `handle.close()`** ✓ |
| `execve` 杀兄弟线程 | `sys/execve.rs` | 对每个 `exited` 调 `vfs::fd::drop_task_fd_table` ✓ |
| 共享 fd 表最后持有者退出 | `drain_task_fd_table` | `ref_count` 归零时 `take_table_handles` + 外层 close ✓ |

### 3.3 引用复制（非新 Pipe）

| 操作 | 文件 | 行为 |
|------|------|------|
| `dup` / `dup3` | `registry.rs` | `VfsIoHandle::duplicate()` → `PipeEndpoint::clone()` → `acquire_read`/`acquire_write` +1 |
| `fork` | `copy_fd_table_from_parent` | 对每个 fd `duplicate()`，pipe 引用计数递增 ✓ |
| `clone` 线程 | `share_fd_table_from_parent` | 共享 fd 表，**不**额外 `acquire_*`（同一 `PipeEndpoint` 实例） |

### 3.4 `Drop` / 隐式销毁

| 类型 | `Drop` 实现 | 风险 |
|------|------------|------|
| `PipeEndpoint` | **无** | 若不经 `close()` 直接 drop，`read_refs`/`write_refs` 不减；依赖 `Arc` 归零释放 `Pipe` 内存，但 `read_open`/`write_open` 可能仍为 true |
| `Pipe` / `PipeState` | `Arc` 释放 | `WaitQueue` 句柄销毁，但 **未** 调用 `try_release_empty` 回收 `WaitQueueId` |
| `PipeReadHandle`/`PipeWriteHandle` | **无** | 同上，须走 `VfsIoHandle::close` |

### 3.5 内部未走 `close()` 的路径（风险点）

| 路径 | 文件 | 问题 |
|------|------|------|
| `PerTaskFdRegistry::drop_task_fd_table` | `registry.rs:527` | `close_table` 仅 `take_table_handles`，**不**调 `handle.close()` |
| 调用方 | `copy_fd_table_from_parent`、`share_fd_table_from_parent` 在子任务已有 fd 表时 | 见 §6 问题 PIPE-P0-01 |

> **注**：正常任务退出走 `vfs::fd::drop_task_fd_table`（外层补 `close()`），与内部 `registry.drop_task_fd_table` 不同。

---

## 4. 生命周期状态机

### 4.1 Pipe 对象（`Arc<Pipe>`）

```
[未分配]
    │ Pipe::with_capacity / Pipe::new
    ▼
[已分配·两端打开]  read_open=true, write_open=true, read_refs≥0, write_refs≥0
    │ acquire_* (pair/dup/fork duplicate/clone)
    ▼
[使用中]            缓冲可读/写；阻塞者挂在 read_wait / write_wait
    │ release_* 归零（close fd）
    ├─ read_refs→0  → read_open=false  → wake write_wait
    └─ write_refs→0 → write_open=false → wake read_wait（读者得 EOF）
    │ 所有 Arc 持有者释放
    ▼
[已释放]            Pipe/Vec/WaitQueue 句柄 drop；WaitQueueId 未必回收到 free 列表
```

### 4.2 PipeEndpoint / fd 句柄

```
pair() 创建
    → 读/写各 1 个 PipeEndpoint，各持 Arc<Pipe>
    → 初始 read_refs=1, write_refs=1（来自 pair 内 acquire）
dup/fork duplicate
    → clone → acquire_* +1（每多一个 fd 引用）
close(fd)
    → release_* -1；归零关闭对应方向
任务退出（正常路径）
    → drop_task_fd_table → handle.close() → release_*
```

### 4.3 半初始化状态

| 场景 | 状态 | 处理 |
|------|------|------|
| `pipe2` 读 fd 分配成功、写 fd `EMFILE` | 读 fd 已入表 | 回滚关闭读 fd ✓ |
| `pipe2` fd 已分配、`copy_to_user` 失败 | 两端已在表，用户不知 fd 号 | Linux 同构；见 §5.3 |
| `Pipe::with_capacity` 失败 | 无对象 | 返回 `Err`，无 partial alloc |

---

## 5. 账本稳定性

### 5.1 结论：**部分稳定**

| 维度 | 结论 | 说明 |
|------|------|------|
| 正常 `close` / 任务退出 | **稳定** | `vfs::fd::drop_task_fd_table` 与 `sys_close` 均调用 `handle.close()` → `release_*` |
| `dup` / `fork` 引用计数 | **稳定** | `clone`/`duplicate` 与 `close` 成对 |
| 内部 `registry.drop_task_fd_table` | **不可靠** | 不经 `close()`，破坏端点引用账本（§6 PIPE-P0-01） |
| `Arc` vs `read_refs`/`write_refs` | **部分脱节** | 无 `Drop` 守卫；纯 `Arc` 归零可释放内存，但逻辑打开状态可能不一致 |
| `WaitQueueId` | **泄漏** | 每 pipe 2 ID，销毁时不 `try_release_empty`（§6 PIPE-P1-01） |
| double-free | **无** | `close` 从 fd 表 `take` 槽位；`release_*` 有 `>0` 检查 |
| UAF | **低风险** | 单核 + fd 表锁；`Arc` 保护 `Pipe` 体；阻塞路径在持锁外 sleep |

### 5.2 与 Linux 语义对齐点

- 空管道 + 写端关闭 → `read` 返回 0（EOF）✓  
- 读端关闭 + 写端仍打开 → 后续 `write` → `EPIPE`/`BrokenPipe` ✓  
- 部分 close 后 dup 持有端仍可读写（`impl-ringbuf` 自检覆盖）✓  
- `fork` 后父子共享 pipe 数据通道 ✓  
- `O_NONBLOCK` 经 `fcntl(F_SETFL)` 写入 `Cell<bool>`（2026-06-25 已收敛）✓  

### 5.3 `pipe2` + `EFAULT` 说明

`copy_to_user(pipefd)` 失败时 syscall 返回 `-EFAULT`，但 **fd 已安装**。这与 Linux `do_pipe_flags` 一致（先 `install_fd` 再拷贝用户指针）。从用户视角 fd 号不可见，属「匿名占用」而非内核表项泄漏；长期滥用可逼近 `EMFILE`。

---

## 6. 耗尽与失败处理

| 资源 | 上限 | 耗尽行为 | 与 Linux 差距 |
|------|------|----------|--------------|
| 每 pipe 缓冲 | 4096 B | 满则写阻塞 / 非阻塞 `EAGAIN` | Linux 默认页大小级（通常 64KiB），容量更小 |
| pipe 个数 | 无 | 受堆 + fd 表间接限制 | Linux 有 `pipe_inode_info` 缓存但无硬 cap |
| fd 槽 | `RLIMIT_NOFILE` | `EMFILE` | 一致 |
| 堆 | 128 MiB | **`alloc_error_handler` → panic** | Linux 返回 `-ENOMEM` |
| `WaitQueueId` | `wait_queues.len()` 单调增 | 无失败返回；仅 ID 复用失败时扩容 Vec | 应随 Pipe 销毁回收 |

**不应静默继续的路径**：`InvalidCapacity` 已拒绝；堆耗尽当前 **panic** 而非 `-ENOMEM`（堆资源审计组 `#6` 交叉项）。

---

## 7. 跨资源耦合

| 事件 | 与 pipe 的交互 |
|------|----------------|
| `fork` | `copy_fd_table_from_parent` → `duplicate()` 递增 `read_refs`/`write_refs`；子表独立 |
| `clone` 线程 | 共享 fd 表；同一 `PipeEndpoint`；`fcntl` 改 `O_NONBLOCK` 影响所有线程（同 Linux file description） |
| `execve` | CLOEXEC pipe 关闭；兄弟线程 `drop_task_fd_table` |
| `exit` / `exit_group` | `drop_task_fd_table` 关闭全部 pipe fd，唤醒阻塞对端 |
| `poll`/`ppoll` | `poll_revents` + `poll_wait_for_ticks`；`poll_engine.rs` 专路径 |
| `socketpair` | 2×Pipe、4×端点；生命周期与本审计相同 |
| 页缓存 / 块缓存 | **无耦合** |
| 锁 | `Pipe.state` 使用 `spin::Mutex`；阻塞在 `WaitQueue` 上释放 CPU，不持 pipe 锁睡眠（见 `lock-inventory` 待补登记） |

---

## 8. 潜在问题清单

### P0（泄漏 / UAF / 卡死）

| ID | 类型 | 描述 | 位置 |
|----|------|------|------|
| **PIPE-P0-01** | **卡死** | `copy_fd_table_from_parent` / `share_fd_table_from_parent` 在子任务已有 fd 表时调用 **`registry.drop_task_fd_table`**，仅 `take_table_handles` **不** `handle.close()`。若子任务持有 pipe（或其它需 `release_*` 的端点），`read_refs`/`write_refs` 与 `read_open`/`write_open` 不更新，对端可在空读/满写上 **永久阻塞**。 | `registry.rs:461–464`, `501–504`, `527–532` |
| **PIPE-P0-02** | **卡死** | 若 pipe 在 **仍有阻塞读者/写者** 时因异常路径销毁 `Arc<Pipe>`（未经 `close()` 唤醒），`WaitQueue` 无 `Drop` 唤醒逻辑；阻塞者依赖调度器 `detach_task_from_run_queues`（任务退出时）解除。若任务 **不退出** 仅 fd 表被 `close_table` 无声丢弃，可永久睡眠。为 PIPE-P0-01 的加重情形。 | `kernel_pipe.rs`, `wait_queue.rs`, `registry.rs` `close_table` |

### P1（错误路径 / 资源泄漏 / 语义偏差）

| ID | 类型 | 描述 | 位置 |
|----|------|------|------|
| **PIPE-P1-01** | **泄漏** | 每个 `Pipe` 创建 2 个 `WaitQueueId`，销毁时未 `try_release_empty`；`wait_queues` Vec **只增不减**（除非别处复用 free 列表）。大量 `pipe2`/`socketpair` 后调度器等待队列表膨胀。 | `kernel_pipe.rs:258–264`, `wait_queue.rs` |
| **PIPE-P1-02** | **静默耗尽** | `Pipe::new()` / `vec![0; capacity]` 堆分配失败 → **panic**，非 `-ENOMEM`。 | `runtime-heap-allocator`, `kernel_pipe.rs` |
| **PIPE-P1-03** | **无上限** | 无全局「已创建 pipe 数」或「pipe 缓冲总字节」限额；仅受 128MiB 堆与 1024 fd 间接约束。 | 全局 |
| **PIPE-P1-04** | **防御缺失** | `PipeEndpoint` / `PipeReadHandle` 无 `Drop` → `close()`；依赖外层 fd 表契约。内部路径一旦 drop 句柄即破坏引用账本。 | `endpoint.rs`, `handles.rs` |
| **PIPE-P1-05** | **API 不一致** | `KernelPipe::close_read`/`close_write` 直接改 `read_open`/`write_open`，**不**维护 `read_refs`/`write_refs`；当前 VFS 仅用 `release_*`，但 trait 双路径易误用。 | `kernel_pipe.rs:353–361` |

### P2（限额 / 错误码 / 文档）

| ID | 类型 | 描述 |
|----|------|------|
| **PIPE-P2-01** | 语义 | 默认缓冲 4096B，小于 Linux 常见 64KiB；大吞吐测试可能更多唤醒次数 |
| **PIPE-P2-02** | 伪 inode | `NEXT_PIPE_INODE` 单调递增，不复用；仅 `metadata()` 用，无内核账本影响 |
| **PIPE-P2-03** | 文档 | `docs/exports/features/wateros-ipc.md` 仍写「fork/dup 继承待联调」，实际已实现 `copy_fd_table_from_parent` + `duplicate` |
| **PIPE-P2-04** | 交叉 | `pipe2`/`socketpair` `copy_to_user` 失败时 fd 已创建（Linux 一致）；可在审计文档标注避免误报为泄漏 |

---

## 9. 收敛建议

1. **PIPE-P0-01**：`registry.drop_task_fd_table` 改为与 `vfs::fd::drop_task_fd_table` 相同——先 `take_table_handles` 再对每个 `handle.close()`；或禁止在 `copy_fd_table_from_parent` 中使用无声 `drop`，改为 `drain` + close。
2. **PIPE-P1-01**：为 `Pipe` 实现 `Drop`，在 `read_wait`/`write_wait` 上调用 `try_release_empty()`（须先 `wake_all` 确保队列为空，或文档化仅空队列释放）。
3. **PIPE-P1-04**：`PipeEndpoint` 实现 `Drop { self.close() }` 作为安全网（注意与显式 `close` 双重调用：当前 `release_*` 在 refs=0 后重复调用无害）。
4. **PIPE-P1-02/03**：在 `pipe_handle_pair` 入口增加堆预检或捕获 OOM 返回 `VfsError::OutOfMemory` → `-ENOMEM`；可选全局 `PIPE_MAX_COUNT` + `warn!`。
5. 不可靠路径统一：`log::warn!` 含 `pipe`、`read_refs`/`write_refs`、`task_id`、`fd`。

---

## 10. 修复任务草案

| 优先级 | 标题 | 文件 | 验收标准 |
|--------|------|------|----------|
| P0 | fork 前清理子 fd 表须 close pipe 端点 | `vfs-impl/impl-fd-session/src/registry.rs` | `copy_fd_table_from_parent` 前后：子进程仅持 pipe 写端、父阻塞写满管道；fork 重置子表后对端不永久阻塞 |
| P0 | 统一 `drop_task_fd_table` 与 `drain` 语义 | `registry.rs`, `fd.rs` | 删除或收敛内部无声 `close_table`；self_test 改用 `drain`+`close` |
| P1 | Pipe 销毁回收 WaitQueueId | `kernel_pipe.rs` | 创建/销毁 1000 根 pipe 后 `wait_queues.len()` 不线性增至 2000+（ID 复用） |
| P1 | PipeEndpoint `Drop` 守卫 | `endpoint.rs` | 句柄 `drop` 未显式 `close` 时 `read_refs` 正确递减 |
| P2 | 文档同步 fork/dup pipe 继承 | `docs/exports/features/wateros-ipc.md` | 与 `wateros-vfs.md` / 本审计一致 |
| P2 | 可选 pipe 全局计数上限 | `handles.rs` 或 `ipc.rs` config | 超限 `warn!` + `pipe2` 返回 `-ENOSPC` 或 `-ENOMEM` |

---

## 11. 调用链速查

### 分配（`pipe2`）

```
sys_pipe2
  → vfs::pipe_handle_pair(nonblocking)
      → PipeEndpoint::pair
          → Arc::new(Pipe::new())
          → PipeState::with_capacity(4096)  // Vec 堆分配
          → WaitQueue::new() ×2
          → acquire_read / acquire_write
  → PerTaskFdRegistry::alloc_fd_for_task ×2
  → copy_to_user(pipefd)
```

### 回收（`close`）

```
sys_close
  → vfs::fd::close_fd
      → take_fd_for_close
      → PipeReadHandle::close / PipeWriteHandle::close
          → PipeEndpoint::close
              → Pipe::release_read / release_write
                  → refs==0 → read_open/write_open=false → wake_all
      → drop handle Box（Arc 可能仍存活）
```

### 回收（任务退出）

```
drop_task_runtime_resources
  → vfs::fd::drop_task_fd_table
      → drain_task_fd_table → Vec<Handle>
      → for handle in handles { handle.close() }
```

---

## 12. 账本稳定性总评

| 资源 | 评级 |
|------|------|
| #20 Pipe + PipeEndpoint | **部分稳定** — 主路径（`close`、任务退出、`dup`/`fork`）正确；内部 `registry.drop_task_fd_table` 与无 `Drop` 守卫为缺口 |
| #21 环形缓冲 `Vec<u8>` | **稳定** — 随 `Arc<Pipe>` 生命周期释放；无独立泄漏路径 |

**综合**：常规 syscall 路径可用于生产测例；**fork 前子表重置**与 **WaitQueueId 回收**为优先修复项，与 LTP 后期卡死/资源膨胀场景相关。
