# 系统调用语义审计：G01–G07（I/O 与 fd）

> **审计组**：G01–G07 + `sendfile`  
> **基线**：Linux asm-generic 64 位 syscall 语义  
> **事实来源**：`syscall-impl/impl-kernel/src/sys/*`、`wateros-vfs`、`wateros-ipc` pipe、`driver-network`  
> **审计日期**：2026-06-25

---

## 1. 组概述

本组覆盖用户态最基础的 **字节流 I/O** 与 **文件描述符生命周期** 操作，是 shell、BusyBox、网络栈与测试程序（LTP/iozone）的高频路径。

| 子组 | syscall | nr | 实现文件 | 路由 |
|------|---------|-----|----------|------|
| G01 | `read`, `readv` | 63, 65 | `sys/read.rs` | `dispatch_read` / `dispatch_readv` |
| G02 | `write`, `writev` | 64, 66 | `sys/write.rs` | `dispatch_write` / `dispatch_writev` |
| G03 | `pread64`, `pwrite64`, `preadv`, `pwritev` | 67–70 | `sys/posix_at_io.rs` | `dispatch_pread64` … `dispatch_pwritev` |
| G04 | `dup`, `dup3` | 23, 24 | `sys/dup.rs` | `dispatch_dup` / `dispatch_dup3` |
| G05 | `pipe2`, `close` | 59, 57 | `sys/pipe2.rs`, `sys/close.rs` | `dispatch_pipe2` / `dispatch_close` |
| G06 | `ioctl` | 29 | `sys/ioctl.rs`, `sys/rtc.rs` | `dispatch_ioctl` |
| G07 | `fcntl` | 25 | `sys/fcntl.rs` | `dispatch_fcntl` |
| — | `sendfile` | 71 | `sys/sendfile.rs` | `dispatch_sendfile` |

**组内共性**：

- 普通 VFS fd 经 `vfs::fd::with_current_io`（take/restore 句柄，持锁窗口外执行 I/O）；socket fd 经 `socket_fd` 旁路表 + `driver::network::stack`。
- 错误码统一经 `vfs_util::vfs_error_to_errno` / `vfs_io_at_error_to_errno` 映射。
- **阻塞语义分裂**：pipe 使用 `WaitQueue` 真阻塞；socket 使用有界 tick 自旋（默认 128–4096 tick）；TTY/stdin 多为立即 EOF 或 `EINVAL`。
- **用户缓冲**：经 `user_copy` 做有限校验，非完整 Linux `access_ok` 模型。

---

## 2. 各 syscall 审计

### 2.1 `read`（nr 63）

| 项 | 内容 |
|----|------|
| **分发入口** | `KernelSyscallDispatcher::dispatch_read` → `sys_read` |
| **实现文件** | `syscall-impl/impl-kernel/src/sys/read.rs` |
| **Linux 语义要点** | 从 fd 读至多 `count` 字节；`count==0` 返回 0；阻塞 fd 无数据时睡眠；`EINTR`/`EAGAIN` 可恢复；pipe/socket/TCP 语义各异 |
| **当前覆盖** | **部分** — VFS 文件/pipe/设备；socket 旁路；stdin 无真实交互输入 |

**可靠性分析**

- 参数：`len==0` → 0；`ptr==0` → `EFAULT`；`len>4MiB` → `EINVAL`（Linux 无此硬顶，属 WaterOS 收敛）。
- VFS 路径：单次 `read_fd` 调用，pipe 阻塞由底层 `KernelPipe::read` + `WaitQueue` 完成（**不卡死**）。
- Socket 路径：`read_tcp_socket_blocking` / `read_udp_socket_blocking` 最多 `SOCKET_READ_WAIT_TICKS`（4096）次 `sleep_for_ticks(1)`，超时返回 `EAGAIN`（**非 Linux 无限阻塞**）。
- Stdin：`CharDevHandle::new_stdin`（`stdin_eof=true`）在驱动无数据时返回 **EOF(0)**；无串口时用 `ConsoleInHandle` 恒返回 0。

**潜在问题**

| 严重度 | 问题 |
|--------|------|
| P1 | 阻塞 socket `read` 有界等待后返回 `EAGAIN`，与 Linux 无限阻塞不一致；长连接读可能意外失败 |
| P1 | 串口 TTY（非 stdin_eof）驱动无数据时 `read_serial_tty` → `VfsError::Unsupported` → **`EINVAL`**，应为 `EAGAIN` 或阻塞 |
| P2 | `len` 上限 4MiB 拒绝大缓冲 benchmark |
| P2 | 文档写「stdin EBADF」，实际为 **立即 EOF(0)**，与 `wateros-syscall.md` 快照不一致 |

**收敛建议**

- Socket 阻塞读：在 tick 耗尽前 `log::warn!("[sys_read] fd={} socket blocking read timed out after {} ticks", fd, wait_ticks)` → 保持 `EAGAIN` 或实现真阻塞。
- TTY 无数据：`warn` + `EAGAIN`（非阻塞）或接入 `poll_wait_for_ticks`。
- Stdin 无输入源：`warn` 一次 + 文档明确 **EOF 语义**（非 EBADF）。

---

### 2.2 `readv`（nr 65）

| 项 | 内容 |
|----|------|
| **分发入口** | `dispatch_readv` → `sys_readv` |
| **实现文件** | `sys/read.rs` |
| **Linux 语义要点** | 按 `iovec` 顺序填充；短读返回已读总量；中途错误且已有数据时返回已有字节数 |
| **当前覆盖** | **部分** — 与 `read` 共享 `read_fd` |

**可靠性分析**

- `iovcnt==0` → 0；`iov_ptr==0` → `EFAULT`；`iovcnt>1024` → `EINVAL`。
- 逐 iov 调用 `read_fd`：短读（`n < iov.len`）正确提前返回；错误时 `total>0` 返回 success(total)（**符合 Linux**）。
- 每个 iov 独立分配内核缓冲，大 `iovcnt` 有分配压力。

**潜在问题**

| 严重度 | 问题 |
|--------|------|
| P1 | 继承 `read` 的 socket 有界阻塞与 TTY `EINVAL` 问题 |
| P2 | 调试 `log::trace!` 每 iov 一条，iozone 场景日志膨胀 |

**收敛建议**

- 与 `read` 同步修复底层 `read_fd`。
- 生产构建降低 `readv` trace 级别。

---

### 2.3 `write`（nr 64）

| 项 | 内容 |
|----|------|
| **分发入口** | `dispatch_write` → `sys_write` |
| **实现文件** | `sys/write.rs` |
| **Linux 语义要点** | 写至多 `count` 字节；pipe 满时阻塞；写已关闭 pipe 读端 → `EPIPE` + `SIGPIPE`；短写合法 |
| **当前覆盖** | **部分** — 控制台/VFS/pipe/socket |

**可靠性分析**

- 参数校验同 `read`；用户数据先完整拷入内核缓冲再 `write_fd`。
- `EPIPE` 时调用 `raise_current_thread(SIGPIPE)` 并返回 `EPIPE`（**符合 Linux**）。
- Socket 写：最多 `SOCKET_WRITE_WAIT_TICKS`（128）次等待，超时 `EAGAIN`。
- VFS pipe 写：`PipeWriteHandle::write` 真阻塞（`WaitQueue`）。

**潜在问题**

| 严重度 | 问题 |
|--------|------|
| P1 | 阻塞 socket `write` 128 tick 后 `EAGAIN`，非 Linux 语义 |
| P2 | 大 `len` 先整段 `copy_from_user` 再写，短写时仍全量拷贝 |

**收敛建议**

- Socket 写超时：`warn` + 考虑延长或真阻塞；与 `SO_SNDTIMEO` 对齐。

---

### 2.4 `writev`（nr 66）

| 项 | 内容 |
|----|------|
| **分发入口** | `dispatch_writev` → `sys_writev` |
| **实现文件** | `sys/write.rs` |
| **Linux 语义要点** | 按序扫描 iov 写入；返回总写字节数（可小于请求总量） |
| **当前覆盖** | **部分** |

**可靠性分析**

- 先将所有 iov **拼接为一个内核 `Vec`**，再单次 `write_fd`。
- 若 `write_fd` 短写，返回短写长度；**未写回的 iov 剩余在内核缓冲中丢弃** — 与 Linux「按 iov 边界短写」在极端情况下行为可能不同（通常 `write_fd` 一次写全或短写开头）。

**潜在问题**

| 严重度 | 问题 |
|--------|------|
| P1 | 继承 socket 有界写等待 |
| P2 | 全量 gather 后短写：对超大 `writev` 语义边界与 Linux 不完全一致 |

**收敛建议**

- 大 `writev` 按 iov 分段写以贴近 Linux 短写语义。

---

### 2.5 `pread64`（nr 67）

| 项 | 内容 |
|----|------|
| **分发入口** | `dispatch_pread64` → `sys_pread64` |
| **实现文件** | `sys/posix_at_io.rs` |
| **Linux 语义要点** | 在 `offset` 处读，**不改变** fd 文件偏移；pipe/socket/seek 不可句柄 → `ESPIPE` |
| **当前覆盖** | **已接入**（常规文件） |

**可靠性分析**

- 负 offset（高 bit 符号扩展）→ `EINVAL`。
- `VfsIoHandle::read_at`；`Unsupported` → `ESPIPE`（`vfs_io_at_error_to_errno`）。
- 不改变 fd 当前 offset（由 `read_at` 实现保证）。

**潜在问题**

| 严重度 | 问题 |
|--------|------|
| P2 | 仅常规文件/可 seek 句柄；socket/pipe 正确 `ESPIPE` |

**收敛建议**

- 保持；对非 seek 句柄可在入口 `warn` 一次（可选）。

---

### 2.6 `pwrite64`（nr 68）

| 项 | 内容 |
|----|------|
| **分发入口** | `dispatch_pwrite64` |
| **实现文件** | `sys/posix_at_io.rs` |
| **Linux 语义要点** | 同 `pread64`，写方向；只读 FS → `EROFS` |
| **当前覆盖** | **已接入** |

**可靠性分析**

- 与 `pread64` 对称；`write_at` + 错误映射。

**潜在问题**

| 严重度 | 问题 |
|--------|------|
| P2 | 无 |

**收敛建议**

- 无。

---

### 2.7 `preadv`（nr 69）

| 项 | 内容 |
|----|------|
| **分发入口** | `dispatch_preadv` |
| **实现文件** | `sys/posix_at_io.rs` |
| **Linux 语义要点** | 在 `offset` 起按 iov 散布读入 |
| **当前覆盖** | **部分** — 功能可用，实现策略不同 |

**可靠性分析**

- 先 `total_iov_len` 算总需求，再 64KiB 分块 `read_at` 累积到 `gathered`，最后 `scatter_to_user_iovecs`。
- 每调用打 `log::info!`（**生产噪声**）。
- 内存峰值 ≈ 实际读取总量（非 Linux 内核页缓存直 scatter）。

**潜在问题**

| 严重度 | 问题 |
|--------|------|
| P2 | 大块 `preadv` 双倍内存（gather + scatter） |
| P2 | `info!` 级日志每条 syscall |

**收敛建议**

- 降为 `trace!` 或 feature gate。
- 大 IO 考虑直接 scatter 到 iov，避免中间 `Vec`。

---

### 2.8 `pwritev`（nr 70）

| 项 | 内容 |
|----|------|
| **分发入口** | `dispatch_pwritev` |
| **实现文件** | `sys/posix_at_io.rs` |
| **Linux 语义要点** | gather 用户 iov 后从 `offset` 连续写入 |
| **当前覆盖** | **已接入** |

**可靠性分析**

- `gather_user_iovecs` + 单次 `write_at`；空 iov → 0。

**潜在问题**

| 严重度 | 问题 |
|--------|------|
| P2 | 超大 iov 全量 gather 内存峰值 |

**收敛建议**

- 与 `preadv` 类似，大 IO 分块写。

---

### 2.9 `dup`（nr 23）

| 项 | 内容 |
|----|------|
| **分发入口** | `dispatch_dup` → `sys_dup` |
| **实现文件** | `sys/dup.rs` |
| **Linux 语义要点** | 复制到 ≥0 的最小可用 fd；与旧 fd 共享文件表项；新 fd **不**继承 `FD_CLOEXEC` |
| **当前覆盖** | **已接入** |

**可靠性分析**

- `vfs::fd::dup_fd(oldfd, 0)` + `handle.duplicate()`；新 fd flags 置 0。
- Socket：同步 `socket_fd::register_with_flags` 共享 `SocketRef`。
- 无效 `oldfd` → `EBADF`；`EMFILE` 由 registry 检查。

**潜在问题**

| 严重度 | 问题 |
|--------|------|
| P2 | nr **23** 与不可达 `select` 哨兵冲突（见 `syscall-inventory.md` 路由说明）— 非 `dup` 实现缺陷 |

**收敛建议**

- 文档标注 nr 23 陷阱；可选兼容层检测错误 `select` 调用。

---

### 2.10 `dup3`（nr 24）

| 项 | 内容 |
|----|------|
| **分发入口** | `dispatch_dup3` → `sys_dup3` |
| **实现文件** | `sys/dup.rs` |
| **Linux 语义要点** | 复制到指定 `newfd`；`flags` 仅 `O_CLOEXEC`；`oldfd==newfd` 且 flags 含 `O_CLOEXEC` → `EINVAL`；`oldfd==newfd` 且 flags==0 → 成功返回 `newfd` |
| **当前覆盖** | **部分** |

**可靠性分析**

- 非法 flags → `EINVAL`；`oldfd==newfd` → **入口直接 `EINVAL`**（与 Linux flags==0 成功 **不符**）。
- `newfd` 已打开时先关闭（registry `close_slot`）；socket 表同步 `remove` + `register`。
- 支持 `O_CLOEXEC`。

**潜在问题**

| 严重度 | 问题 |
|--------|------|
| P1 | `dup3(fd, fd, 0)` Linux 合法，WaterOS **恒 `EINVAL`** |
| P2 | 未支持 `O_CLOEXEC` 以外的 flag（已拒绝，正确） |

**收敛建议**

- `oldfd==newfd && flags==0`：委托 registry 已有逻辑，返回 `newfd`；`flags&O_CLOEXEC` 时保持 `EINVAL`。
- `warn` 仅在 flags 非法时。

---

### 2.11 `pipe2`（nr 59）

| 项 | 内容 |
|----|------|
| **分发入口** | `dispatch_pipe2` → `sys_pipe2` |
| **实现文件** | `sys/pipe2.rs` |
| **Linux 语义要点** | 创建 pipe；`flags` 支持 `O_NONBLOCK`、`O_CLOEXEC`（可组合） |
| **当前覆盖** | **部分** — 仅 `O_NONBLOCK` |

**可靠性分析**

- `pipefd_ptr==0` → `EFAULT`。
- `flags & !O_NONBLOCK` → `EINVAL`（**拒绝 `O_CLOEXEC`**）。
- 创建 `vfs::pipe_handle_pair(nonblocking)`，分配连续 fd，写回用户 `int[2]`。
- fork 后 `copy_fd_table_from_parent` 继承（文档化能力）。

**潜在问题**

| 严重度 | 问题 |
|--------|------|
| P1 | 常用 `pipe2(fd, O_CLOEXEC)` 被拒绝 → `EINVAL`，影响安全 spawn 模式 |
| P2 | 未设置新 fd 的 `FD_CLOEXEC` 位（即使将来接受 flag，registry 需同步） |

**收敛建议**

- 实现 `O_CLOEXEC`：`alloc` 后 `set_fd_flags(fd, FD_CLOEXEC)`；非法 flag `warn!("[sys_pipe2] unsupported flags={:#x}", flags)` → `EINVAL`。

---

### 2.12 `close`（nr 57）

| 项 | 内容 |
|----|------|
| **分发入口** | `dispatch_close` → `sys_close` |
| **实现文件** | `sys/close.rs` |
| **Linux 语义要点** | 关闭 fd；释放描述符；已关闭/无效 → `EBADF` |
| **当前覆盖** | **已接入** |

**可靠性分析**

- `vfs::fd::close_fd` 调用句柄 `close()`（pipe 端点 `release_read/write`）。
- Socket / unix socket 旁路表清理。
- 无效 fd → `EBADF`。

**潜在问题**

| 严重度 | 问题 |
|--------|------|
| P2 | 无显著卡死或静默错误 |

**收敛建议**

- 无。

---

### 2.13 `ioctl`（nr 29）

| 项 | 内容 |
|----|------|
| **分发入口** | `dispatch_ioctl` → `sys_ioctl` |
| **实现文件** | `sys/ioctl.rs`, `sys/rtc.rs` |
| **Linux 语义要点** | 设备相关控制；TTY `TCGETS`/`TIOCGWINSZ`/`TIOCGPGRP`；RTC `RTC_RD_TIME`/`RTC_SET_TIME`；未知 request → `ENOTTY`/`EINVAL` |
| **当前覆盖** | **部分** — TTY/RTC 子集 + VFS 句柄 `ioctl` |

**可靠性分析**

- 路由顺序：RTC fd → TTY fd → `handle.ioctl` → RTC fallback → `global_ioctl_fallback`。
- `TCGETS` 故意 **`ENOTTY`**（避免写 termios 覆盖用户栈金丝雀，代码注释明确）。
- `TIOCGWINSZ` 固定 80×25；`TIOCGPGRP` 返回当前 task id；`TIOCNOTTY` no-op 成功。
- RTC：`realtime_ns` / `set_realtime_ns`；非法时间 → `EINVAL`。
- 未知 request → **`ENOTTY`**（非 `EINVAL`）。

**潜在问题**

| 严重度 | 问题 |
|--------|------|
| P1 | 大量块设备/套接字 `ioctl` 未实现 → `ENOTTY`，用户态可能误判 |
| P2 | `TCGETS` 拒绝导致部分程序 fallback 路径依赖 `TIOCGWINSZ` 桩 |
| P2 | 非 TTY 非 RTC fd 的 VFS `ioctl` 默认 `Unsupported` → 走 fallback 仍 `ENOTTY` |

**收敛建议**

- 高频未实现 request：`warn!("[sys_ioctl] fd={} req={:#x} argp={:#x} ENOTTY", …)`。
- 文档列出已支持 request 白名单；其余明确 `ENOTTY`。

---

### 2.14 `fcntl`（nr 25）

| 项 | 内容 |
|----|------|
| **分发入口** | `dispatch_fcntl` → `sys_fcntl` |
| **实现文件** | `sys/fcntl.rs` |
| **Linux 语义要点** | `F_DUPFD`/`F_DUPFD_CLOEXEC`/`F_GETFD`/`F_SETFD`/`F_GETFL`/`F_SETFL`（`O_NONBLOCK` 等）；未知 cmd → 传统上部分返回 `EINVAL` |
| **当前覆盖** | **部分** |

**可靠性分析**

- 已实现：0–4、1030（`F_DUPFD_CLOEXEC`）。
- `F_GETFD`/`F_SETFD`：registry `FD_CLOEXEC`。
- `F_GETFL`：socket 返回 `O_RDWR|O_NONBLOCK`；**普通文件恒 `O_RDWR`，忽略真实 open flags**。
- `F_SETFL`：仅 socket 可设 `O_NONBLOCK`；**pipe/普通 fd 静默成功 no-op**。
- 未知 cmd → **`ENOSYS`**。

**潜在问题**

| 严重度 | 问题 |
|--------|------|
| P1 | `F_SETFL` 对 pipe 设 `O_NONBLOCK` **静默无效**，`read` 仍阻塞 — 语义陷阱 |
| P1 | `F_GETFL` 对文件 fd 返回假 flags |
| P1 | 未知 cmd `ENOSYS` — glibc 对部分 cmd 期望 `EINVAL` |
| P2 | `F_SETFL` 非 socket 路径 `Ok(0)` 无 warn |

**收敛建议**

- Pipe：`F_SETFL` 应更新 `PipeEndpoint.nonblocking` 或 registry 状态；暂不可则 `warn!("[sys_fcntl] F_SETFL fd={} flags={:#x} pipe nonblock not implemented", …)` → `EINVAL`。
- 未知 cmd：`warn` + 统一 `EINVAL`（与 Linux 现代行为对齐）。
- 文件 `F_GETFL`：从 open flags 或 VFS 元数据读取。

---

### 2.15 `sendfile`（nr 71）

| 项 | 内容 |
|----|------|
| **分发入口** | `dispatch_sendfile` |
| **实现文件** | `sys/sendfile.rs` |
| **Linux 语义要点** | `in_fd` → `out_fd` 内核拷贝；`offset` 可选；`in_fd` 须可 seek 或配合 `splice`；支持 socket 出端 |
| **当前覆盖** | **部分** — 仅 VFS 句柄，64KiB 缓冲循环 |

**可靠性分析**

- `in_fd==out_fd` → `EINVAL`；`count==0` → 0。
- `offset_ptr!=0` 用 `read_at`/`pwrite` 语义更新 `*offset`；否则 `read` 推进 in_fd 偏移。
- **不经过 `socket_fd` 路径** — socket `out_fd`/`in_fd` 可能 `EBADF` 或 VFS 错误。
- 短写/读 EOF 正确处理 `transferred`。

**潜在问题**

| 严重度 | 问题 |
|--------|------|
| P1 | **Socket fd 不支持**，nginx/零拷贝路径失败 |
| P2 | pipe 作 `in_fd` 时 `read_at` → `ESPIPE`（符合 Linux） |
| P2 | 纯用户态语义下无 `splice` 零拷贝 |

**收敛建议**

- Socket 出端：`warn!("[sys_sendfile] out_fd={} is socket, not supported", out_fd)` → `EINVAL` 或 `ENOSYS`。
- 入口检测 socket/pipe 组合并明确错误码。

---

## 3. 组内共性问题汇总

### 3.1 阻塞与卡死

| 现象 | 影响 syscall | 说明 |
|------|--------------|------|
| Pipe 真阻塞（`WaitQueue`） | read/write/pipe2 | **安全**，与 Linux 一致 |
| Socket 有界 tick 自旋后 `EAGAIN` | read/write/readv/writev | 长阻塞语义偏差，**非无限卡死** |
| Stdin 立即 EOF(0) | read | 交互脚本读不到输入，**不阻塞** |
| TTY 无数据 → `EINVAL` | read | 错误码不符，可能导致重试风暴 |

### 3.2 静默成功 / 假语义

| 路径 | 风险 |
|------|------|
| `fcntl(F_SETFL)` 非 socket | 返回 0 但不改变 pipe 阻塞属性 |
| `fcntl(F_GETFL)` 非 socket | 返回固定 `O_RDWR` |
| `ioctl(TIOCNOTTY)` | 故意 no-op（可接受） |
| `dup3(fd,fd,0)` | 错误拒绝（应成功） |

### 3.3 错误码策略

| 映射 | 说明 |
|------|------|
| `VfsError::Unsupported`（普通 read） | → `EINVAL`（TTY 读） |
| `VfsError::Unsupported`（at-io） | → `ESPIPE` |
| `fcntl` 未知 cmd | → `ENOSYS` |
| `ioctl` 未知 request | → `ENOTTY` |

### 3.4 旁路双轨（VFS vs socket_fd）

`read`/`write`/`dup`/`fcntl` 识别 socket 旁路表；**`sendfile`、`pread*`/`pwrite*` 仅 VFS**。socket 与常规文件混用时常出现 `EBADF`/`ESPIPE`/`ENOTSOCK` 不一致。

### 3.5 参数硬顶

多处 `4MiB`/`1024 iov` 限制 — WaterOS 收敛，与 Linux 不同，benchmark 可能 hit `EINVAL`。

---

## 4. 本组 P0/P1 问题条目（供主 agent 汇总）

### P0

*本组未发现典型「无限阻塞/内核 panic」的 P0 路径；pipe 阻塞为设计内真睡眠。原清单「stdin 卡死」与当前实现（立即 EOF）不符，降级为 P2 文档偏差。*

### P1

1. **`read`/`readv` 阻塞 socket 读**：4096 tick 后 `EAGAIN`，非 Linux 无限等待。
2. **`write`/`writev` 阻塞 socket 写**：128 tick 后 `EAGAIN`。
3. **`fcntl(F_SETFL)` 对 pipe/VFS fd 静默 no-op**：设 `O_NONBLOCK` 无效，与后续 `read` 阻塞组合为语义陷阱。
4. **`fcntl(F_GETFL)` 非 socket 返回假 flags**。
5. **`dup3(oldfd, oldfd, 0)` 错误返回 `EINVAL`**（Linux 应成功）。
6. **`pipe2` 不支持 `O_CLOEXEC`**：常见 spawn 模式失败。
7. **`sendfile` 不支持 socket fd**：仅 VFS，网络场景失败。
8. **`ioctl` 大量 request 未实现**：统一 `ENOTTY`，易误导用户态。
9. **TTY 无数据时 `read` 返回 `EINVAL`**（应为 `EAGAIN` 或阻塞）。

---

*单组审计完成。路径：`docs/audits/syscall/io-fd.md`*
