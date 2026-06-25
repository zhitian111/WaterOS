# 系统调用支持范围说明（文档 B）

> **生成时间**：2026-06-25  
> **Baseline**：Linux asm-generic 64 位 ABI（`LinuxGeneric64`，RISC-V / LoongArch 共用）  
> **对照对象**：当前 `KernelSyscallDispatcher` + `dispatch_unknown` 旁路 + `trap_handler` 特殊路径  
> **详细问题**：见 [`syscall-issues.md`](syscall-issues.md)

---

## 1. 覆盖总览

| 维度 | 说明 |
|------|------|
| **已注册 ABI 槽位** | 139 常量（138 可解码 nr + `SELECT` 哨兵 `usize::MAX`） |
| **另有旁路 nr** | 8 个（`dispatch_unknown`） |
| **trap 特殊** | `rt_sigreturn`(139) |
| **`sys_*` 实现数** | 146（含不可解码的 `sys_select`） |
| **相对完整 Linux** | 子集；大量 flag/组合为部分实现或 stub |

### 1.1 实现状态图例

| 标记 | 含义 |
|------|------|
| ✅ | 主路径可用，与 Linux 大体一致 |
| ⚠️ | 部分实现：核心路径可用，flag/边界有缺口 |
| 🔶 | stub：校验后成功或 no-op，不持久化真实语义 |
| 🚫 | 明确拒绝：返回错误码（非 panic） |
| ❌ | 未实现 / 仅 `ENOSYS`（号表外 nr） |
| 🔀 | 旁路 nr（`dispatch_unknown`） |
| ⚡ | trap 特殊路径 |

---

## 2. 按类别覆盖矩阵

### 2.1 I/O 与文件描述符

| syscall | nr | 状态 | Linux 语义覆盖要点 | 主要缺口 |
|---------|-----|------|-------------------|----------|
| read | 63 | ⚠️ | VFS/pipe/socket；短读 | socket 有界阻塞；TTY `EINVAL`；stdin EOF |
| readv | 65 | ⚠️ | 同 read | 同上 |
| write | 64 | ⚠️ | VFS/pipe/socket；EPIPE+SIGPIPE | socket 有界阻塞 |
| writev | 66 | ⚠️ | 同 write | 同上 |
| pread64 | 67 | ✅ | `read_at` | pipe/socket → ESPIPE |
| pwrite64 | 68 | ✅ | `write_at` | 同上 |
| preadv | 69 | ✅ | iovec + at | 4MiB 上限 |
| pwritev | 70 | ✅ | 同上 | 同上 |
| sendfile | 71 | ⚠️ | 文件→文件/socket；64KiB 缓冲 | 无零拷贝；socket 旁路有限 |
| dup | 23 | ✅ | `dup_fd` | — |
| dup3 | 24 | ⚠️ | `dup3_fd` | `dup3(fd,fd,0)` 与 Linux 不符 |
| pipe2 | 59 | ⚠️ | pipe + `O_NONBLOCK` | 无 `O_CLOEXEC` |
| close | 57 | ✅ | 关闭 fd | — |
| ioctl | 29 | ⚠️ | TTY/RTC 子集 | 大量 `ENOTTY` |
| fcntl | 25 | ⚠️ | DUPFD/GETFL/SETFL/CLOEXEC 等 | 未知 cmd `ENOSYS`；pipe O_NONBLOCK no-op |

### 2.2 VFS 与路径

| syscall | nr | 状态 | 主要缺口 |
|---------|-----|------|----------|
| openat | 56 | ⚠️ | 不 follow symlink；大量 open flag 忽略 |
| faccessat | 48 | ⚠️ | 三参 ABI；无 owner 检查 |
| faccessat2 | 439 | 🔀⚠️ | 旁路；`AT_SYMLINK_NOFOLLOW` 未生效 |
| fchmodat | 53 | ⚠️ | 基本路径 |
| fchownat | 54 | ⚠️ | uid/gid 写 VFS 有限 |
| readlinkat | 78 | ⚠️ | 基本读链接 |
| statfs | 43 | ⚠️ | 假容量数据 |
| fstat | 80 | ⚠️ | 128B kstat；uid/gid=0 |
| fstatat | 79 | 🔀⚠️ | 旁路 |
| statx | 291 | 🔀⚠️ | 旁路；字段子集 |
| lseek | 62 | ⚠️ | pipe → ESPIPE |
| getdents64 | 61 | ⚠️ | 须 `O_DIRECTORY` |
| mkdirat | 34 | ⚠️ | 仅 `AT_FDCWD` |
| symlinkat | 36 | ⚠️ | 基本创建 |
| unlinkat | 35 | ⚠️ | RO 卷 EROFS |
| renameat2 | 276 | ⚠️ | 仅 flags=0 同目录 |
| utimensat | 88 | 🔶 | 内存旁路表，不持久化 |
| sync/fsync/fdatasync | 81–83 | ⚠️ | flush 路径；锁序风险 |
| ftruncate | 46 | ⚠️ | 基本 |
| fallocate | 47 | ⚠️ | 部分 mode |
| mount | 40 | ⚠️ | MS_RDONLY 等子集；同步重操作 |
| umount2 | 39 | ⚠️ | 忽略 flags |
| getcwd | 17 | ⚠️ | 256B 内核缓冲 |
| chdir | 49 | ✅ | `chdir_current` | — |

### 2.3 进程、执行与调度

| syscall | nr | 状态 | 主要缺口 |
|---------|-----|------|----------|
| exit | 93 | ✅ | 多线程 per-thread exit 语义简化 |
| exit_group | 94 | ✅ | 推荐多线程终止路径 |
| clone/fork | 220 | ⚠️ | flag 未收敛；非 leader fork 风险 |
| clone3 | 435 | ⚠️ | 共用 clone 路径；pidfd 等 ENOSYS |
| execve | 221 | ⚠️ | ELF+shebang；无 env PATH；失败不可恢复 |
| waitpid/wait4 | 260 | ⚠️ | WNOHANG；无 rusage；pid 子集 |
| kill | 129 | ⚠️ | 无进程组 kill |
| sched_yield | 124 | ✅ | — |
| sched_setparam…getaffinity | 118–123 | ⚠️ | setaffinity EPERM；getaffinity 单核 |
| sched_get_priority_max/min | 125–126 | ✅ | — |
| sched_setattr/getattr | 274–275 | 🔀⚠️ | 旁路 |
| getpid/ppid/tid | 172–173, 178 | ⚠️ | orphan ppid=1；tid≈pid |
| setsid | 157 | 🔶 | 无真实会话 |
| setpgid | 154 | 🔶 | 不持久化 |
| set_tid_address | 96 | ⚠️ | 无完整 clear_child_tid 语义文档化 |

### 2.4 内存

| syscall | nr | 状态 | 主要缺口 |
|---------|-----|------|----------|
| brk | 214 | ⚠️ | Sv39/LoongArch；无 aspace 假桩 |
| mmap | 222 | ⚠️ | 匿名/文件映射；无 aspace → panic |
| munmap/mprotect/mremap | 215/226/216 | ⚠️ | 同上；mremap 子集 |
| madvise/msync | 233/227 | 🔶 | no-op 成功 |
| mlock* | 228–231 | 🔶 | no-op 成功 |
| get_mempolicy | 236 | ⚠️ | mm::mempolicy 子集 |
| shmget/ctl/at/dt | 194–197 | ⚠️ | SysV SHM 最小实现 |

### 2.5 时间与计时器

| syscall | nr | 状态 | 主要缺口 |
|---------|-----|------|----------|
| gettimeofday | 169 | ⚠️ | REALTIME + offset |
| clock_settime/gettime/getres | 112–114 | ⚠️ | 时钟 ID 子集 |
| clock_nanosleep/nanosleep | 115/101 | ⚠️ | ~10ms tick 精度 |
| times | 153 | ⚠️ | 最小 tms |
| setitimer/getitimer | 103/102 | ✅ | REAL/VIRTUAL/PROF |
| adjtimex | 171 | 🔀⚠️ | 旁路；不改墙钟 |
| clock_adjtime | 266 | 🔀⚠️ | 旁路 |

### 2.6 身份、凭证与杂项

| syscall | nr | 状态 | 主要缺口 |
|---------|-----|------|----------|
| getuid…setresgid | 174–177, 144–149 | ✅/⚠️ | cred 无权限模型 |
| getgroups | 158 | ⚠️ | G1=[0]；边界可 panic |
| capget/capset | 90–91 | 🔶 | LTP 桩 |
| sysinfo/uname | 179/160 | ⚠️ | 固定/最小字段 |
| prctl | 167 | ⚠️ | 常用 op no-op 子集 |
| getrlimit/setrlimit/prlimit64 | 163–164/261 | ⚠️ | 部分资源 |
| umask/getrusage/getrandom | 166/165/278 | ⚠️ | 伪随机等 |
| syslog | 116 | ✅ | klog；非法指针 panic |
| acct | 89 | 🔀🔶 | 旁路；仅路径校验 |

### 2.7 信号、futex

| syscall | nr | 状态 | 主要缺口 |
|---------|-----|------|----------|
| rt_sigreturn | 139 | ⚡✅ | trap 路径 |
| rt_sigaction/procmask/pending | 134–136 | ⚠️/✅ | 无 SA_RESTORER 读入 |
| rt_sigsuspend/timedwait | 133/137 | ⚠️ | interrupt 依赖 |
| tkill/tgkill | 130–131 | ✅ | 无权限检查 |
| futex | 98 | ⚠️ | WAIT/WAKE 子集；bitset 忽略 |
| set/get_robust_list | 99–100 | ⚠️ | get ABI 错误 |

### 2.8 Socket 与多路复用

| syscall | nr | 状态 | 主要缺口 |
|---------|-----|------|----------|
| socket | 198 | ⚠️ | 地址族子集 |
| socketpair | 199 | ⚠️ | AF_UNIX STREAM |
| bind/listen/accept/connect | 200–203, 242 | ⚠️ | 阻塞语义不一致 |
| getsockname/peername | 204–205 | ⚠️ | — |
| sendto/recvfrom/sendmsg/recvmsg | 206–207, 211–212 | ⚠️ | 超时/EINTR 偏差 |
| setsockopt/getsockopt/shutdown | 208–210 | ⚠️ | opt 子集 |
| ppoll | 73 | ⚠️ | sigmask 未实现 |
| pselect6 | 72 | ⚠️ | 同上 |
| poll | 271 | ⚠️ | ms 超时；轮询 socket |
| select | — | ⚠️ | **号表不可达**；用 pselect6 |

---

## 3. 明确未覆盖（Linux 常见但本内核无 handler）

以下 nr **不在**当前实现范围内，调用返回 `-ENOSYS`（`dispatch_unknown` 默认）：

- 完整 Linux 号表其余数百项
- `clone` 大量 namespace flag（`CLONE_NEW*`）
- 完整 `inotify`、`epoll`、`signalfd`、`timerfd`
- `io_uring`、`seccomp`、`bpf` 等

**注意**：号表槽位若未来新增但未 override `dispatch_*`，将 **panic**（非 ENOSYS）。新增 syscall 必须同时接 handler。

---

## 4. 架构与构建差异

| 项 | RISC-V | LoongArch |
|----|--------|-----------|
| 号表 | `LinuxGeneric64` 相同 | 相同 |
| MM 后端 | Sv39 | LoongArch64 |
| `user_aspace_ptr==0` 时 mmap 族 | panic | panic（**非** ENOSYS） |
| `uname.machine` | `riscv64` | `loongarch64` |
| `rt_sigreturn` trampoline | 架构相关固定地址 | 架构相关固定地址 |

---

## 5. 路由特例登记

| 机制 | 说明 |
|------|------|
| `dispatch_unknown` 旁路 | 79, 89, 171, 266, 274, 275, 291, 439 → 对应 `sys_*` |
| `rt_sigreturn` | `trap_handler.rs` 在分发前处理 |
| `SELECT` 哨兵 | `usize::MAX`；`sys_select` 不可通过标准 nr 到达 |
| nr 23 陷阱 | 表内为 `dup`，非 `select` |

---

## 6. 与导出文档的差异（需同步）

| 导出文档陈述 | 代码实际 | 建议 |
|-------------|----------|------|
| stdin `EBADF` | 多数路径 **EOF(0)** | 更新 `features/wateros-syscall.md` |
| LoongArch mmap `ENOSYS` | 无 aspace 时 **panic** | 更新导出；收敛后改 ENOSYS |
| `select`(23) 已接入 | nr 23 为 **dup** | 更正为 pselect6(72) |

---

## 7. 详细审计索引

| 文档 | 路径 |
|------|------|
| 盘点清单 | [`syscall-inventory.md`](syscall-inventory.md) |
| 问题清单 | [`syscall-issues.md`](syscall-issues.md) |
| I/O 组 | [`syscall/io-fd.md`](syscall/io-fd.md) |
| VFS 组 | [`syscall/vfs-path.md`](syscall/vfs-path.md) |
| 进程组 | [`syscall/process-exec.md`](syscall/process-exec.md) |
| 内存/时间/身份 | [`syscall/mm-time-cred.md`](syscall/mm-time-cred.md) |
| 信号/socket/poll | [`syscall/signal-socket-poll.md`](syscall/signal-socket-poll.md) |
