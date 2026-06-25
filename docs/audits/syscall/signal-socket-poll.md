# 系统调用语义审计：G39–G46（信号 / futex / socket / 多路复用）

> 审计范围：G39–G46  
> Baseline：Linux generic 64-bit ABI（`impl-linux-generic64`）  
> 生成时间：2026-06-25

---

## 1. 概述

本组覆盖线程同步信号、futex、BSD socket 与 `poll` 族多路复用，是 BusyBox/LTP/网络测试中最易出现**阻塞卡死、EINTR 语义偏差、静默错误**的区域。

| 分组 | 编号 | 主要 syscall（riscv64 nr） | 入口 / 实现 |
|------|------|---------------------------|-------------|
| G39 信号 | — | `rt_sigreturn`(139)、`rt_sigaction`(134)、`rt_sigprocmask`(135)、`rt_sigpending`(136)、`rt_sigsuspend`(133)、`rt_sigtimedwait`(137)、`tkill`(130)、`tgkill`(131) | `trap_handler.rs`（sigreturn）、`sys/signal.rs`、`sys/task.rs` |
| G40 futex | — | `futex`(98)、`set_robust_list`(99)、`get_robust_list`(100) | `sys/futex.rs`、`sys/robust.rs`、`ipc-futex/impl-task` |
| G41–G45 socket | — | `socket`(198)…`shutdown`(210) 全套 | `sys/socket*.rs`、`sys/accept.rs`、`sys/connect.rs`、`unix_sock.rs`、`poll_engine.rs`（就绪） |
| G46 多路复用 | — | `ppoll`(73)、`pselect6`(72)、`select`（号表不可达）、`poll`(271) | `sys/poll_multiplex.rs`、`sys/poll.rs`、`poll_engine.rs` |

**分发说明**：`rt_sigreturn` 在 `trap_handler.rs` 中于通用分发前拦截；`select` 在号表中为 `usize::MAX` 哨兵，用户态不可达，实际走 `pselect6`(72)。

---

## 2. G39 信号

### 2.1 `rt_sigreturn` (139)

| 项 | 内容 |
|----|------|
| Linux 语义 | 从信号帧恢复 `ucontext`、信号掩码，返回到被中断的用户态；非法帧应杀进程（`SIGSEGV`/`SIGILL`） |
| 实现 | `trap_handler.rs` 检测 nr==139 → `restore_signal_frame` → `return_to_user_signal_delivery`；失败 `kill_current_user_task` |
| 覆盖 | 已实现：magic 校验、`SignalMachineContext` 恢复、mask 写回 registry |
| 与 Linux 差异 | `SA_RESTART` 仅对 `restartable_syscall` 白名单 syscall 生效（含 `accept4`/`connect`/`recvfrom` 等，**不含** `ppoll`/`poll`/`futex`） |

**潜在问题**

| 严重度 | 问题 |
|--------|------|
| P1 | `rt_sigaction` 写入时 `restorer` 恒为 0；依赖固定 trampoline 地址（`0x7FFF_B000` / LoongArch 变体），自定义 `SA_RESTORER` 用户态可能不兼容 |
| P2 | 信号帧仅 64-bit `sigset`；`sigset_size != 8` 的路径在其他 syscall 已拒绝，一致 |

### 2.2 `rt_sigaction` / `rt_sigprocmask` / `rt_sigpending`

| syscall | 状态 | 要点 |
|---------|------|------|
| `rt_sigaction` | 部分 | per-thread registry；`sig==0`/`SIGKILL`/`SIGSTOP` 等由 registry 拒绝；`SA_RESTORER` 未从用户读入 |
| `rt_sigprocmask` | 已实现 | `how` 仅支持 block/unblock/setmask；`set==NULL` 只读 oldmask |
| `rt_sigpending` | 已实现 | 合并 thread+process pending；`sigset_size` 必须 8 |

### 2.3 `rt_sigsuspend` (133) — **重点**

| 项 | 内容 |
|----|------|
| Linux 语义 | 原子替换掩码并睡眠，直至**可投递**信号；返回 `-1`/`EINTR`；返回前恢复调用前掩码（经 `end_sigsuspend` 或信号帧） |
| 实现 | `begin_sigsuspend` → `WaitQueue::wait_current_while(!has_deliverable)` → `end_sigsuspend` → 恒 `EINTR` |
| 可工作路径 | 掩码外已有 pending 信号时 `has_deliverable` 立即为真，不睡眠；`tkill`/`tgkill` + `interrupt_task` 可打断 waitqueue |

**潜在问题**

| 严重度 | 问题 | 收敛建议 |
|--------|------|----------|
| **P0** | 睡眠期间依赖 `interrupt_task` 唤醒 waitqueue；若信号已 pending 但**未调用** `interrupt_task`（仅改 registry），可能睡满一轮调度才醒来 | 在 `send_thread`/`apply_signal_dispatch(Pending)` 路径保证对目标 task `interrupt_task`；审计所有 `send_*` 入口 |
| P1 | 存在 LTP 专用 `ltp_fuzz_sigsuspend_worker_fast_exit_if_standalone()` 短路，非通用语义 | 文档标注；生产路径不受影响 |
| P1 | 被信号打断后恒返回 `EINTR`，不区分「无信号错误唤醒」 | 与 Linux 一致，可接受 |
| P2 | `has_deliverable` 不区分 `SIGSTOP`/`SIGCONT` 特殊投递 | 首版可忽略 |

### 2.4 `rt_sigtimedwait` (137)

| 项 | 内容 |
|----|------|
| 状态 | 已实现 |
| 实现 | `begin_signal_wait` + 带 deadline 的 `wait_current_while`；超时 `EAGAIN`；中断 `EINTR` |
| 差异 | 超时基于 `monotonic_ns` 与调度 tick 向上取整，精度 ~`SCHED_TIMER_PERIOD_MS` |

### 2.5 `tkill` / `tgkill`

| 项 | 内容 |
|----|------|
| 状态 | 已实现 |
| 语义 | `signal==0` 存在性探测；`tgkill` 校验 `tgid==pid` |
| 问题 P2 | 未实现跨进程权限检查（`CAP_KILL`/`same_user_ns`），测试环境通常无影响 |

---

## 3. G40 futex 与 robust list

### 3.1 `futex` (98) — **重点**

| 项 | 内容 |
|----|------|
| Linux 语义 | `WAIT`/`WAKE`/`REQUEUE`/`CMP_REQUEUE`/`WAIT_BITSET`/`WAKE_BITSET`；`EAGAIN`/`ETIMEDOUT`/`EINTR`；`FUTEX_PRIVATE_FLAG` 区分私有/共享键 |
| 已实现 op | `WAIT`、`WAIT_BITSET`、`WAKE`、`WAKE_BITSET`、`REQUEUE`、`CMP_REQUEUE` |
| 未实现 | 其余 cmd → `ENOSYS` |
| 等待核心 | `FutexHub::wait_while`：读用户 `*uaddr` 比对 → waitqueue 睡眠 → 被 `wake`/`interrupt` 唤醒后返回 |

**潜在问题**

| 严重度 | 问题 | 收敛建议 |
|--------|------|----------|
| **P0** | **无超时 `FUTEX_WAIT`**（`timeout==NULL`）永久睡眠；若配对 `wake` 因 **key 不一致**（private vs shared、`FUTEX_WAIT_BITSET` 与 `WAKE` 混用）丢失，线程永久卡死 | `wake_user_addr` 已对 `clear_child_tid` 双 key 唤醒；其余路径在 wake 失败时打 warn；文档要求 glibc 与内核约定一致使用 `FUTEX_PRIVATE_FLAG` |
| **P0** | `FUTEX_WAIT_BITSET` 的 **`bitset` 参数被忽略**（`let _ = bitset`）；`FUTEX_WAKE_BITSET` 同样忽略 | 对 `bitset != 0xffffffff` 打 warn 并返回 `ENOSYS`，或实现 bitset 过滤 |
| P1 | 相对超时 `timespec` 经 `ns_duration_to_ticks` **最小 1 tick**，亚 tick 超时偏长 | 接受或改为 `ETIMEDOUT` 立即返回 |
| P1 | `FUTEX_CLOCK_REALTIME` 支持不完整路径依赖 `realtime_ns()`，失败 `EIO` | 明确文档 |
| P2 | 睡眠中信号中断 → `EINTR`（经 `TaskWaitResult::Interrupted`） | 与 Linux 一致；需确认所有信号投递都 `interrupt_task` |

### 3.2 `set_robust_list` / `get_robust_list`

| syscall | 状态 | 问题 |
|---------|------|------|
| `set_robust_list` | 已实现 | `len` 必须 `ROBUST_LIST_HEAD_SIZE`；`head` 可读校验 |
| `get_robust_list` | **ABI 错误** | Linux 签名为 `(pid, head**, len*)` **3 参数**；当前实现仅 `(head*, len*)` **2 参数**，**缺少 `pid`**，与 libc 调用约定不兼容 |
| `robust_exit_cleanup` | 部分 | 线程退出遍历链表设 `FUTEX_OWNER_DIED` 并 `wake_all`；**仅 `is_private: true` key**；`list_op_pending` 跳过 |

| 严重度 | 问题 | 收敛建议 |
|--------|------|----------|
| **P0** | `get_robust_list` **参数布局错误**，libc 调用会误把 `pid` 当指针 → `EFAULT`/乱读 | 修正为 3 参数 ABI，或入口检测并 `ENOSYS` + warn |
| P1 | robust wake 不尝试 `is_private: false` | 退出清理时双 key wake（同 `wake_user_addr`） |

---

## 4. G41–G45 Socket

### 4.1 总览

| syscall | nr | 状态 | 实现文件 |
|---------|-----|------|----------|
| `socket` | 198 | 部分 | `socket.rs` — `AF_INET` TCP/UDP、`AF_UNIX` stream/dgram |
| `socketpair` | 199 | 部分 | `socketpair.rs` — 仅 `AF_UNIX`+`SOCK_STREAM`（VFS 双端点） |
| `bind` / `listen` | 200/201 | 部分 | `bind.rs`、`listen.rs` + `unix_sock.rs` |
| `accept` / `accept4` | 202/242 | 部分 | `accept.rs` |
| `connect` | 203 | 部分 | `connect.rs` |
| `getsockname` / `getpeername` | 204/205 | 部分 | `sockname.rs` |
| `sendto` / `recvfrom` | 206/207 | 部分 | `sendto.rs`、`recvfrom.rs` |
| `sendmsg` / `recvmsg` | 211/212 | 部分 | `sendmsg.rs` |
| `setsockopt` / `getsockopt` | 208/209 | 部分 | `sockopt.rs` + smoltcp 栈 |
| `shutdown` | 210 | 部分 | `shutdown.rs` |

**不支持**：`AF_INET6`、原始套接字、`SOCK_SEQPACKET`（INET）、`socketpair` UDP、多数 `setsockopt` 级别。

### 4.2 阻塞语义 — **accept / connect 重点**

#### `accept` / `accept4`

```
非阻塞或已有 pending accept → 立即 accept
否则：for i in 0..wait_ticks { drive_network_stack(); sleep 1 tick; 每 16 tick 查信号→EINTR }
wait_ticks 默认 4096（或 SO_RCVTIMEO 换算）
超时无连接 → EAGAIN（非 ETIMEDOUT）
```

| 严重度 | 问题 |
|--------|------|
| **P0** | **阻塞 `accept` 非无限等待**：默认最多 ~4096 个调度 tick 后返回 `EAGAIN`，与 Linux「永久阻塞直到连接/信号」不符；慢速网络下表现为偶发失败而非卡死 |
| P1 | 客户端地址写死 `127.0.0.1`，端口 0；仅测试可接受 |
| P1 | `EINTR` 检测间隔 16 tick，响应延迟 |
| P2 | `restartable_syscall` 含 `Accept4`，`SA_RESTART` 可重启 |

#### `connect`

```
非阻塞 → EINPROGRESS
阻塞：for 0..256 ticks { poll stack; may_send+is_connected → 0; sleep 1 }
失败 → ETIMEDOUT（非 EAGAIN/EINTR）
```

| 严重度 | 问题 |
|--------|------|
| **P0** | **阻塞 `connect` 无 `EINTR`**：等待循环不检查 `has_deliverable`，信号到达时可能**卡满 256 tick** 才 `ETIMEDOUT` |
| **P0** | **256 tick 硬超时**后 `ETIMEDOUT`，非 Linux 无限等待；长延迟连接失败 |
| P1 | 未实现 `SO_ERROR`/`getsockopt` 查询异步连接错误 |

### 4.3 收发与其他

| 路径 | 行为 | 问题 |
|------|------|------|
| `recvfrom` TCP 阻塞 | 最多 `wait_ticks`（默认 4096 或 `SO_RCVTIMEO`），超时 `EINTR`（TCP）/`EAGAIN`（UDP） | P1：TCP 超时返回 `EINTR` 而非 `EAGAIN`，与 Linux 不一致 |
| `sendto` TCP 阻塞 | 256 tick 后 `EAGAIN` | P1 |
| `sendmsg`/`recvmsg` | 聚合 iovec；逻辑同 sendto/recvfrom | P2：忽略多数 flags |
| `setsockopt` | 主要 `SO_RCVTIMEO`/`SO_SNDTIMEO` 等少量 | 其余 `EOPNOTSUPP` |
| `shutdown` | TCP 支持；UDP `EOPNOTSUPP` | 已实现 |
| `unix_sock` stream `accept` | `loop { sleep 1 }` **无上限** | P1：真正无限阻塞，与 INET accept 不一致 |

### 4.4 `poll_engine` 与 socket 就绪

- 每次扫描调用 `drive_network_stack()` + `poll_socket_events()`。
- TCP `POLLIN`/`POLLOUT` 基于 smoltcp 状态；`poll_block_*` 对 **纯 socket 集合**不走 pipe wait，靠 `sleep_for_ticks(1)` 轮询（CPU 友好性一般，但不应死锁）。

---

## 5. G46 多路复用

### 5.1 号表与可达性

| syscall | nr | 可达 | 说明 |
|---------|-----|------|------|
| `ppoll` | 73 | ✓ | `sys_ppoll` → `poll_engine` |
| `pselect6` | 72 | ✓ | `fd_set` 扫描 + 阻塞 |
| `select` | `usize::MAX` | **✗** | 号表哨兵；`decode` 不可匹配真实 nr；用户态用 `pselect6` |
| `poll` | 271 | ✓ | `timeout` 为毫秒 |

`impl-kernel` 已覆盖 `dispatch_select`/`dispatch_poll`；`syscall-api` 默认 stub 为 `ENOSYS`，由 impl 覆盖。

### 5.2 共享引擎行为

```
scan_* → 有就绪或 deadline 到期 → 返回
否则 poll_wait_*_fds（仅非 socket fd，pipe 等）
  → any_pipe=false 时 sleep_for_ticks(1)
```

- **常规文件**：`poll_revents` 恒就绪（`POLLIN|POLLOUT`）。
- **pipe 读端空且写端打开**：`poll_revents` 无 `POLLIN`；`poll_wait_read` 阻塞。
- **超时**：`timespec`/`timeval`/`poll` ms 均换算为 **调度 tick**（向上取整）。

### 5.3 `ppoll` — **空 pipe 重点**

| 场景 | 预期 Linux 行为 | 当前行为 |
|------|----------------|----------|
| 单 fd：空 pipe 读端，`POLLIN`，`timeout=NULL` | 阻塞至有数据/关写端/信号 | `poll_wait_pipe_fds` → `read_wait`；每轮最多 1 tick 切片等待；**应能阻塞** |
| `nfds==0`，`timeout=NULL` | 永久睡眠 | `sleep_for_ticks(1)` 循环，**符合** |
| `sigmask` 非 NULL | 原子替换掩码 | **已校验但未应用**（`let _ = sigmask_ptr`） |
| `with_current_io` 等待期间 fd 暂离表 | — | 已避免在 wait 条件内重扫同 fd（防 `POLLNVAL` 忙等，见 `poll_engine.rs` 注释） |

**潜在问题**

| 严重度 | 问题 | 收敛建议 |
|--------|------|----------|
| **P0** | 历史问题：`with_current_io` 持锁等待时重扫同 fd → `POLLNVAL` → **忙等卡死**；代码已加注释修复 | 回归测试：`ppoll` 单空 pipe + 另一线程 `write` |
| **P0** | **`sigmask` 未实现**：`ppoll`/`pselect6` 等待期间信号仍可投递，与 Linux 原子语义不符；与 `rt_sigsuspend`/测试用例交叉时行为诡异 | 实现临时掩码（可复用 `begin_sigsuspend` 模式）或 warn + 文档 |
| P1 | 仅 socket fd 时靠 1-tick sleep 轮询，延迟与 CPU 开销高 | 为 socket 增加 waitqueue 或统一 `poll_block` 事件源 |
| P1 | `poll_wait_pipe_fds` 忽略非 `Interrupted` 的 `with_current_io` 错误，`any_pipe` 保持 false | 对 `EBADF` 等返回 `POLLNVAL` 或明确 errno |
| P2 | `select` 不写回剩余 `timeval`（导出文档已注明） | 保持文档一致 |

### 5.4 `pselect6` / `poll`

- 与 `ppoll` 共享 `poll_block_fd_sets`；socket 路径额外 `yield` 自旋 `SOCKET_READY_YIELD_SPINS` 次。
- `poll`(271)：`timeout_ms<0` 无限；`==0` 立即返回。

---

## 6. 卡死风险矩阵（汇总）

| 路径 | 机制 | 严重度 | 典型触发 |
|------|------|--------|----------|
| `FUTEX_WAIT` 无超时 | waitqueue 永久睡眠 | **P0** | mutex 配对错误、private/shared key 不一致、bitset 混用 |
| `ppoll` 空 pipe（回归） | `POLLNVAL` 忙等 | **P0**（已修） | 多线程同 fd `ppoll`+`read` 竞态 |
| `ppoll`/`pselect6` 无 sigmask | 信号与等待竞态 | **P0** | 多线程 pthread + `ppoll` |
| `rt_sigsuspend` | waitqueue 未 interrupt | **P0** | pending 信号但未 `interrupt_task` |
| `connect` 阻塞 | 256 tick 硬等 + 无 EINTR | **P0** | 慢连接 + 信号处理 |
| `accept` 阻塞 | 4096 tick 后 `EAGAIN` | P0/P1 | 长连接队列；表现为失败非卡死 |
| `unix` stream `accept` | 无限 `sleep` 循环 | P1 | 无 listen 端时真卡死 |
| 纯 socket `ppoll` | 1-tick 轮询 | P2 | 高负载 CPU 空转 |

---

## 7. 收敛建议（按优先级）

### P0 — 建议尽快处理

1. **修正 `get_robust_list` ABI**（3 参数）或显式 `ENOSYS`。
2. **`connect` 阻塞循环**：加入 `has_deliverable` → `EINTR`；取消或大幅延长 256 tick 硬上限（改依 `SO_SNDTIMEO` 或无限）。
3. **`ppoll`/`pselect6` 实现 `sigmask`** 或入口 warn + 返回 `ENOSYS`（若短期不实现）。
4. **`futex` `FUTEX_WAIT_BITSET`**：实现 bitset 或拒绝非全 bitset 并 warn。
5. **信号投递路径审计**：凡使 `has_deliverable==true` 的操作必须 `interrupt_task`。

### P1 — 语义补齐

1. `accept` 阻塞改为无限等待（或仅受 `SO_RCVTIMEO` 约束），超时返回 `EAGAIN`/`EINTR` 与 Linux 对齐。
2. `recvfrom` TCP 超时 errno 统一为 `EAGAIN`。
3. `robust_exit_cleanup` 双 key wake。
4. socket-only `poll` 降低轮询开销。

### P2 — 文档/测试

1. 标注 `select`(23) 不可达、`AF_INET6`/`SOCK_RAW` 不支持。
2. 补充回归：`ppoll` 空 pipe、`futex` wait/wake、`rt_sigsuspend`+`kill`、`accept`+`SIGALRM`。

---

## 8. 参考代码位置

| 主题 | 文件 |
|------|------|
| sigreturn 陷阱 | `os/src/trap_handler.rs` L150–160, L327–339 |
| sigsuspend | `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/signal.rs` L366–400 |
| futex wait | `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/futex.rs` L72–96 |
| futex hub | `os/components/wateros-ipc/ipc-futex/futex-impl/impl-task/src/hub.rs` |
| get_robust_list | `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/robust.rs` L151–169 |
| accept 阻塞 | `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/accept.rs` L77–105 |
| connect 阻塞 | `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/connect.rs` L73–86 |
| ppoll / poll 引擎 | `os/components/wateros-syscall/syscall-impl/impl-kernel/src/poll_engine.rs` |
| syscall 号表 | `os/components/wateros-abi/abi-impl/impl-linux-generic64/src/lib.rs` |

---

## 9. P0 / P1 摘要

### P0（卡死 / 严重语义）

1. **`futex` 永久等待**：无超时 wait 遇 wake key 不匹配 → 死锁。
2. **`get_robust_list` ABI 错误**：2 参数 vs Linux 3 参数。
3. **`connect` 阻塞**：无 `EINTR`、256 tick 假超时。
4. **`ppoll` `sigmask` 未实现**：与 Linux 原子等待语义不符，易与信号类测试交叉失败。
5. **`rt_sigsuspend`**：依赖 `interrupt_task`，缺失时延迟唤醒或表现如卡死。
6. **`FUTEX_WAIT_BITSET` bitset 忽略**：条件变量/ pthread 部分路径唤醒丢失 → 死锁。

### P1（错误码 / 边界 / 性能）

1. **`accept` 阻塞有 tick 上限**（4096），最终 `EAGAIN` 非永久等。
2. **`recvfrom` TCP 超时返回 `EINTR`** 而非 `EAGAIN`。
3. **`robust` 清理仅 private key**。
4. **纯 socket `poll` 轮询** 延迟与 CPU。
5. **`rt_sigaction` 忽略用户 restorer**。
6. **`unix` `accept` 真无限阻塞** 与 INET 不一致。
