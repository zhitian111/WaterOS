# 决赛系统调用功能分类与分工

> **目标**：CAgent（200分） + BuildStorm（200分）全量通过  
> **平台**：RISC-V64 / LoongArch64，glibc 环境  
> **基线**：当前已实现 146 个 `sys_*`，需修复 P0/P1 缺陷 + 补充缺失功能

---

## 分工总览

| 角色 | 负责人 | 负责分类 | Syscall 组 | 估分权重 |
|------|--------|---------|------------|----------|
| **A — 进程与信号** | 成员A | 任务/进程/调度/信号/futex | G21–G28, G39–G40 | ~35% |
| **B — 文件与VFS** | 成员B | 文件I/O/VFS/路径/procfs | G01–G20 | ~40% |
| **C — 内存/网络/杂项** | 成员C | 内存/时间/凭证/网络/poll | G29–G38, G41–G46 | ~25% |

> 各组独立工作，但 **procfs**（归 B 组）是所有组的前置依赖——CAgent 一半测试靠它。

---

## A 组：进程、调度、信号、futex（成员A）

### 覆盖测例

| 测例 | 依赖 A 组能力 |
|------|--------------|
| CAgent: factorial | `execve`(bash) + `waitpid` |
| CAgent: date | `clock_gettime`(C 组协助) + `execve`(date) |
| CAgent: cpu | `execve`(nproc/cat) |
| CAgent: kernel | `execve`(uname) |
| BuildStorm: 工具链 | `clone`(多线程) + `execve`(rustc/cargo) + `waitpid` |
| BuildStorm: 编译 | 多线程调度 + futex 同步 + 信号处理 |

### G21–G28：进程与调度

| Syscall | nr | 当前状态 | 优先级 | 需要做的工作 |
|---------|-----|---------|--------|------------|
| exit | 93 | ✅ 已接入 | — | 无需改动 |
| exit_group | 94 | ✅ 已接入 | — | 无需改动 |
| **clone / fork** | 220 | ⚠️ P0-01/02 | **🔴 P0** | 限制非 leader fork；flag 白名单校验 |
| **clone3** | 435 | ⚠️ | **🔴 P0** | 同上，共用 clone 路径 |
| **execve** | 221 | ⚠️ P0-03 | **🔴 P0** | shebang 解析(`#!`)、PATH 搜索、失败回滚保护 |
| **waitpid / wait4** | 260 | ⚠️ P0-01 | **🔴 P0** | 修复非 leader 子进程唤醒；WNOHANG + WEXITED + WCLONE |
| waitid | 95 | ⚠️ | 🟡 P1 | 补齐 flag 支持 |
| kill | 129 | ✅ | — | 无需改动 |
| tkill | 130 | ✅ | — | 无需改动 |
| tgkill | 131 | ✅ | — | 无需改动 |
| sched_yield | 124 | ✅ | — | 无需改动 |
| sched_setparam | 118 | ✅ | — | 无需改动 |
| sched_setscheduler | 119 | ✅ | — | 无需改动 |
| sched_getscheduler | 120 | ✅ | — | 无需改动 |
| sched_getparam | 121 | ✅ | — | 无需改动 |
| **sched_setaffinity** | 122 | ⚠️ 恒 EPERM | 🟡 P1 | 实现真实 CPU 亲和性设置 |
| **sched_getaffinity** | 123 | ⚠️ 单核 | 🟡 P1 | 支持多核掩码 |
| sched_get_priority_max | 125 | ✅ | — | 无需改动 |
| sched_get_priority_min | 126 | ✅ | — | 无需改动 |
| sched_setattr | 274 | 🔀 旁路 | 🟢 P2 | 可选完善 |
| sched_getattr | 275 | 🔀 旁路 | 🟢 P2 | 可选完善 |
| getpid | 172 | ✅ | — | 无需改动 |
| getppid | 173 | ✅ | — | 无需改动 |
| gettid | 178 | ✅ | — | 无需改动 |
| setsid | 157 | ⚠️ | 🟡 P1 | session 持久化 |
| setpgid | 154 | ⚠️ | 🟡 P1 | 进程组持久化 |
| set_tid_address | 96 | ⚠️ | 🟡 P1 | clear_child_tid futex wake |
| **prctl** | 167 | ⚠️ 部分 | 🟡 P1 | 常用 op 补齐（PR_SET_NAME等） |

### G39–G40：信号与 futex

| Syscall | nr | 当前状态 | 优先级 | 需要做的工作 |
|---------|-----|---------|--------|------------|
| **rt_sigreturn** | 139 | ⚡ trap 路径 | ✅ | 无需改动 |
| **rt_sigaction** | 134 | ✅ 已接入 | 🟡 P1 | flag 补齐(SA_RESTORER等) |
| **rt_sigprocmask** | 135 | ✅ 已接入 | — | 无需改动 |
| rt_sigpending | 136 | ✅ 已接入 | — | 无需改动 |
| **rt_sigsuspend** | 133 | ⚠️ P0-11 | **🔴 P0** | 信号 pending 后 interrupt 唤醒 |
| **rt_sigtimedwait** | 137 | ✅ 已接入 | — | 无需改动 |
| **futex** | 98 | ⚠️ P0-08/09 | **🔴 P0** | FUTEX_PRIVATE 路由；bitset 匹配校验；WAKE 失败尝试 alternate key |
| set_robust_list | 99 | ✅ 已接入 | — | 无需改动 |
| get_robust_list | 100 | ⚠️ ABI 修复 | 🟡 P1 | 修正 3 参数 ABI |

### A 组新增需求

| 新增功能 | 优先级 | 说明 |
|---------|--------|------|
| **shebang 解析** | 🔴 P0 | `execve` 中识别 `#!` 行并加载解释器（bash/perl） |
| **PATH 环境变量搜索** | 🔴 P0 | `execve` 对相对路径按 PATH 搜索 |
| **getpriority/setpriority** | 🟢 P2 | `nice` 值支持（CAgent 可能用到） |

---

## B 组：文件 I/O、VFS 路径、procfs（成员B）

### 覆盖测例

| 测例 | 依赖 B 组能力 |
|------|--------------|
| CAgent: factorial | bash 脚本读取 |
| CAgent: cpu | **`/proc/cpuinfo`** |
| CAgent: kernel | **`/proc/version`** 或 `uname`(A组) |
| CAgent: network | **`/proc/net/tcp`** |
| CAgent: fs-create | `openat`+`write`+`close` |
| CAgent: fs-readwrite | 文件读写 |
| CAgent: fs-directory | `mkdirat`+`getdents64` |
| CAgent: fs-search | `getdents64` 递归 |
| CAgent: fs-usage | `statfs` |
| BuildStorm: 全流程 | 大量文件 I/O + 符号链接 + 目录遍历 |

### G01–G07：文件描述符 I/O

| Syscall | nr | 当前状态 | 优先级 | 需要做的工作 |
|---------|-----|---------|--------|------------|
| **read** | 63 | ⚠️ 部分 | 🟡 P1 | stdin EOF 处理；TTY EAGAIN |
| **readv** | 65 | ⚠️ 部分 | 🟡 P1 | 同 read |
| **write** | 64 | ⚠️ 部分 | 🟡 P1 | EPIPE + SIGPIPE 验证 |
| writev | 66 | ⚠️ 部分 | 🟡 P1 | 同 write |
| pread64 | 67 | ✅ 已接入 | — | 无需改动 |
| pwrite64 | 68 | ✅ 已接入 | — | 无需改动 |
| preadv | 69 | ✅ 已接入 | — | 无需改动 |
| pwritev | 70 | ✅ 已接入 | — | 无需改动 |
| **sendfile** | 71 | ⚠️ 部分 | 🟡 P1 | socket 旁路补齐 |
| dup | 23 | ✅ 已接入 | — | 无需改动 |
| dup3 | 24 | ⚠️ | 🟡 P1 | `dup3(fd,fd,0)` 修复 |
| **pipe2** | 59 | ⚠️ 部分 | 🟡 P1 | 支持 O_CLOEXEC |
| close | 57 | ✅ 已接入 | — | 无需改动 |
| **ioctl** | 29 | ⚠️ 部分 | 🟡 P1 | TTY/FIONBIO/SIOCGIF* 补齐 |
| **fcntl** | 25 | ⚠️ 部分 | 🟡 P1 | F_SETFD/F_GETFD/F_DUPFD 补齐 |

### G08–G20：VFS 与路径操作

| Syscall | nr | 当前状态 | 优先级 | 需要做的工作 |
|---------|-----|---------|--------|------------|
| **openat** | 56 | ⚠️ 部分 | **🔴 P0** | O_NOFOLLOW/O_CLOEXEC/O_EXCL/O_DIRECTORY 补齐；符号链接追踪 |
| faccessat | 48 | ⚠️ 部分 | 🟡 P1 | AT_SYMLINK_NOFOLLOW |
| faccessat2 | 439 | 🔀 旁路 | 🟡 P1 | 同 faccessat |
| fchmodat | 53 | ⚠️ 部分 | 🟡 P1 | 权限检查细化 |
| fchownat | 54 | ⚠️ 部分 | 🟡 P1 | uid/gid 持久化 |
| **readlinkat** | 78 | ⚠️ 部分 | 🟡 P1 | 符号链接读取 |
| symlinkat | 36 | ⚠️ 部分 | 🟡 P1 | 符号链接创建 |
| **statfs** | 43 | ⚠️ 假容量 | **🔴 P0** | 返回真实容量数据（df/du 用） |
| **fstat** | 80 | ⚠️ 部分 | 🟡 P1 | uid/gid 非零；时间戳更新 |
| fstatat | 79 | 🔀 旁路 | 🟡 P1 | 同 fstat |
| statx | 291 | 🔀 旁路 | 🟡 P1 | statx 字段补齐 |
| lseek | 62 | ⚠️ 部分 | 🟡 P1 | pipe→ESPIPE 确认 |
| **getdents64** | 61 | ⚠️ 部分 | **🔴 P0** | 确保非 O_DIRECTORY 返回 ENOTDIR |
| mkdirat | 34 | ⚠️ 部分 | 🟡 P1 | 多级目录支持 |
| unlinkat | 35 | ⚠️ 部分 | 🟡 P1 | AT_REMOVEDIR 验证 |
| renameat2 | 276 | ⚠️ 部分 | 🟡 P1 | RENAME_NOREPLACE 支持 |
| utimensat | 88 | 🔶 stub | 🟡 P1 | 时间戳持久化（非 stub） |
| sync | 81 | ⚠️ 部分 | 🟢 P2 | 无需优先 |
| fsync | 82 | ⚠️ 部分 | 🟡 P1 | 锁序风险修复 |
| fdatasync | 83 | ⚠️ 部分 | 🟡 P1 | 同 fsync |
| ftruncate | 46 | ⚠️ 部分 | 🟡 P1 | 文件截断 |
| fallocate | 47 | ⚠️ 部分 | 🟢 P2 | 可选 |
| mount | 40 | ⚠️ 部分 | 🟡 P1 | MS_RDONLY/MS_BIND 等 |
| umount2 | 39 | ⚠️ 部分 | 🟢 P2 | 可选 |
| **getcwd** | 17 | ⚠️ 部分 | 🟡 P1 | 缓冲区 4096 确认 |
| chdir | 49 | ✅ 已接入 | — | 无需改动 |

### B 组新增 — procfs 伪文件系统

| 文件 | 用于测例 | 优先级 | 数据来源 |
|------|---------|--------|---------|
| **`/proc/cpuinfo`** | CAgent cpu | **🔴 P0** | `RISCV`/`mvendorid` / CPU 核数 |
| **`/proc/version`** | CAgent kernel | **🔴 P0** | `uname` 信息 |
| **`/proc/uptime`** | BuildStorm 计时 | **🔴 P0** | 内核启动时长 |
| **`/proc/net/tcp`** | CAgent network | **🔴 P0** | smoltcp socket 表 |
| `/proc/self/*` | 通用工具 | 🟡 P1 | 进程信息 |
| `/proc/self/maps` | pmap/procmap | 🟡 P1 | 内存映射 |
| `/proc/self/status` | 通用 | 🟡 P1 | 进程状态 |
| `/proc/self/fd/` | lsof | 🟢 P2 | 文件描述符 |
| `/proc/sys/kernel/*` | 系统参数 | 🟢 P2 | 内核参数 |
| `/proc/meminfo` | free/top | 🟢 P2 | 内存信息 |

> **实现建议**：在 VFS 中注册一个只读的 `ProcFs`，按需生成文件内容（类似 Linux `proc_fill_super`）。不需要持久化存储。

### B 组新增 — 其他

| 新增功能 | 优先级 | 说明 |
|---------|--------|------|
| **O_CLOEXEC 全面支持** | 🔴 P0 | execve 时关闭 marked fd |
| **O_NOFOLLOW 支持** | 🔴 P0 | 符号链接不追踪 |
| **O_DIRECTORY 支持** | 🔴 P0 | openat 对目录验证 |
| **AT_FDCWD 全面支持** | 🟡 P1 | 所有 at 系列 syscall |
| **tmpfs 容量返回真实值** | 🟡 P1 | statfs 不返回假数据 |

---

## C 组：内存、时间、凭证、网络、I/O 多路复用（成员C）

### 覆盖测例

| 测例 | 依赖 C 组能力 |
|------|--------------|
| CAgent: date | `clock_gettime`(时间系统调用) |
| CAgent: network | socket TCP + 计时 |
| CAgent: fs-usage | `statfs`(B组) |
| BuildStorm: 全流程 | `mmap`(大量)、`brk`、多线程栈、网络 socketpair |
| BuildStorm: 编译 | 内存映射 + 时间系统调用 |

### G29–G32：内存管理

| Syscall | nr | 当前状态 | 优先级 | 需要做的工作 |
|---------|-----|---------|--------|------------|
| **brk** | 214 | ⚠️ 部分 | **🔴 P0** | 空地址空间时正确处理 |
| **mmap** | 222 | ⚠️ P0-04 | **🔴 P0** | MAP_GROWSDOWN/MAP_STACK/MAP_FIXED/MAP_SHARED 支持；空地址空间避免 panic |
| **munmap** | 215 | ⚠️ 部分 | **🔴 P0** | 同 mmap 保护 |
| mprotect | 226 | ⚠️ 部分 | 🟡 P1 | PROT_NONE/PROT_READ/PROT_WRITE 组合 |
| **mremap** | 216 | ⚠️ 部分 | 🟡 P1 | MREMAP_MAYMOVE/MREMAP_FIXED |
| madvise | 233 | ⚠️ 部分 | 🟡 P1 | MADV_DONTNEED/MADV_WILLNEED |
| msync | 227 | ⚠️ stub | 🟢 P2 | 可暂略 |
| mlock | 228 | 🔶 stub | 🟢 P2 | 可暂略 |
| munlock | 229 | 🔶 stub | 🟢 P2 | 可暂略 |
| mlockall | 230 | 🔶 stub | 🟢 P2 | 可暂略 |
| munlockall | 231 | 🔶 stub | 🟢 P2 | 可暂略 |
| get_mempolicy | 236 | ⚠️ 部分 | 🟢 P2 | 可暂略 |
| shmget | 194 | ⚠️ 部分 | 🟢 P2 | BuildStorm 可能用到 |
| shmctl | 195 | ⚠️ 部分 | 🟢 P2 | |
| shmat | 196 | ⚠️ 部分 | 🟢 P2 | |
| shmdt | 197 | ⚠️ 部分 | 🟢 P2 | |

### G33–G38：时间、凭证、杂项

| Syscall | nr | 当前状态 | 优先级 | 需要做的工作 |
|---------|-----|---------|--------|------------|
| **gettimeofday** | 169 | ⚠️ 部分 | 🟡 P1 | 精度验证 |
| **clock_gettime** | 113 | ⚠️ 部分 | 🟡 P1 | CLOCK_MONOTONIC/CLOCK_PROCESS_CPUTIME_ID |
| clock_settime | 112 | ⚠️ 部分 | 🟢 P2 | 可暂略 |
| clock_getres | 114 | ⚠️ 部分 | 🟢 P2 | |
| **clock_nanosleep** | 115 | ⚠️ 部分 | **🔴 P0** | 精度 + 信号中断 EINTR |
| **nanosleep** | 101 | ⚠️ 部分 | 🟡 P1 | 同 clock_nanosleep |
| times | 153 | ⚠️ 部分 | 🟡 P1 | tms 结构填充 |
| setitimer | 103 | ✅ 已接入 | — | 无需改动 |
| getitimer | 102 | ✅ 已接入 | — | 无需改动 |
| adjtimex | 171 | 🔀 旁路 | 🟢 P2 | |
| clock_adjtime | 266 | 🔀 旁路 | 🟢 P2 | |
| getuid | 174 | ✅ 已接入 | — | 无需改动 |
| geteuid | 175 | ✅ 已接入 | — | 无需改动 |
| getgid | 176 | ✅ 已接入 | — | 无需改动 |
| getegid | 177 | ✅ 已接入 | — | 无需改动 |
| **getgroups** | 158 | ⚠️ P0-06 | **🔴 P0** | 边界参数 panic → EFAULT |
| setuid | 146 | ✅ 已接入 | — | 无需改动 |
| setgid | 144 | ✅ 已接入 | — | 无需改动 |
| setreuid | 145 | ✅ 已接入 | — | 无需改动 |
| setregid | 143 | ✅ 已接入 | — | 无需改动 |
| setresuid | 147 | ✅ 已接入 | — | 无需改动 |
| setresgid | 149 | ✅ 已接入 | — | 无需改动 |
| capget | 90 | ⚠️ 部分 | 🟡 P1 | capabilities 补齐 |
| capset | 91 | ⚠️ 部分 | 🟡 P1 | 同上 |
| sysinfo | 179 | ⚠️ 部分 | 🟡 P1 | 系统信息填充 |
| uname | 160 | ⚠️ 部分 | 🟡 P1 | 版本号真实化 |
| **getrlimit** | 163 | ⚠️ 部分 | 🟡 P1 | RLIMIT_NPROC/RLIMIT_NOFILE 等 |
| setrlimit | 164 | ⚠️ 部分 | 🟡 P1 | 同上 |
| prlimit64 | 261 | ⚠️ 部分 | 🟡 P1 | 跨进程 rlimit |
| umask | 166 | ⚠️ 部分 | 🟡 P1 | 新建文件权限掩码 |
| **getrusage** | 165 | ⚠️ 部分 | 🟡 P1 | RUSAGE_CHILDREN/RUSAGE_SELF |
| getrandom | 278 | ⚠️ 部分 | 🟡 P1 | GRND_NONBLOCK 等 |
| syslog | 116 | ⚠️ P0-07 | **🔴 P0** | 空指针 panic → EFAULT |
| acct | 89 | ⚠️ 部分 | 🟢 P2 | 可暂略 |

### G41–G45：网络 syscall（C 组负责）

| Syscall | nr | 当前状态 | 优先级 | 需要做的工作 |
|---------|-----|---------|--------|------------|
| **socket** | 198 | ✅ 已接入 | 🟡 P1 | SOCK_NONBLOCK/SOCK_CLOEXEC flag |
| socketpair | 199 | ⚠️ 部分 | 🟡 P1 | AF_UNIX SOCK_STREAM 验证 |
| **bind** | 200 | ✅ 已接入 | — | 无需改动 |
| **listen** | 201 | ✅ 已接入 | — | 无需改动 |
| **accept** | 202 | ✅ 已接入 | 🟡 P1 | SOCK_NONBLOCK 验证 |
| accept4 | 242 | ✅ 已接入 | 🟡 P1 | 同上 |
| **connect** | 203 | ✅ 已接入 | 🟡 P1 | 超时/阻塞语义验证 |
| getsockname | 204 | ⚠️ 部分 | 🟡 P1 | 地址结构填充 |
| getpeername | 205 | ⚠️ 部分 | 🟡 P1 | 同上 |
| **sendto** | 206 | ✅ 已接入 | — | 无需改动 |
| **recvfrom** | 207 | ✅ 已接入 | — | 无需改动 |
| sendmsg | 211 | ⚠️ 部分 | 🟡 P1 | iovec 支持验证 |
| recvmsg | 212 | ⚠️ 部分 | 🟡 P1 | 同上 |
| **setsockopt** | 208 | ⚠️ 部分 | 🟡 P1 | TCP_NODELAY/SO_REUSEADDR/SO_RCVTIMEO |
| **getsockopt** | 209 | ⚠️ 部分 | 🟡 P1 | SO_TYPE/SO_ERROR/SO_KEEPALIVE |
| shutdown | 210 | ⚠️ 部分 | 🟡 P1 | SHUT_RD/SHUT_WR/SHUT_RDWR |

### G46：I/O 多路复用

| Syscall | nr | 当前状态 | 优先级 | 需要做的工作 |
|---------|-----|---------|--------|------------|
| **ppoll** | 73 | ⚠️ 部分 | 🟡 P1 | sigmask 原子切换（2026-06 已修✅） |
| pselect6 | 72 | ⚠️ 部分 | 🟡 P1 | 同 ppoll |
| select | — | ✅ 已实现但号表不可达 | 🟡 P1 | 修复号表路由 |
| **poll** | 271 | ⚠️ 部分 | 🟡 P1 | socket/pipe 事件轮询验证 |
| **epoll_create1** | — | ❌ 未实现 | 🟡 P1 | BuildStorm cargo 可能用到 |
| **epoll_ctl** | — | ❌ 未实现 | 🟡 P1 | 同上 |
| **epoll_pwait** | — | ❌ 未实现 | 🟡 P1 | 同上 |

### C 组新增 — epoll 简易实现

epoll 可用 ppoll/poll_engine 代理实现：

```rust
// 简易 epoll 内部用 poll_engine 实现
struct EpollEntry {
    fd: usize,
    events: u32,      // EPOLLIN/EPOLLOUT/EPOLLERR
    data: u64,        // epoll_data
}

// epoll_create1(EPOLL_CLOEXEC) → 返回 epoll fd
// epoll_ctl(EPOLL_CTL_ADD/MOD/DEL, fd, event)
// epoll_pwait(events, maxevents, timeout, sigmask)
```

---

## 依赖关系图

```
procfs (B组) ──────→ CAgent cpu/kernel/network
     │
     ├── 依赖 VFS readdir/openat (B组自身)
     ├── 依赖 smoltcp 状态查询 (C组协作)
     └── 依赖 uname/uptime (C组时间)

execve shebang (A组) ──→ CAgent 全部 shell 脚本
     │
     ├── 依赖 PATH 搜索 (A组)
     ├── 依赖 openat/read (B组, 读脚本文件)
     └── 依赖 mmap (C组, 加载 ELF)

futex bitset (A组) ────→ BuildStorm 多线程同步
     └── 依赖 robust_list (A组)

mmap 增强 (C组) ──────→ BuildStorm 编译
     └── 依赖 brk/mprotect/mremap (C组)
```

---

## 时间线建议（3 人并行）

```
Week 1 (7/21-7/27):
  A: execve shebang + PATH + clone flag 白名单 + futex bitset
  B: procfs (/proc/cpuinfo, /proc/version, /proc/uptime, /proc/net/tcp) + O_CLOEXEC
  C: mmap MAP_GROWSDOWN/MAP_STACK + clock_nanosleep 精度 + getrandom

Week 2 (7/28-8/3):
  A: rt_sigsuspend EINTR + waitpid WCLONE + sched_setaffinity
  B: statfs 真实容量 + openat flag 补齐 + getdents64 验证
  C: epoll 简易实现 + setsockopt/getsockopt 补齐 + getgroups panic 修复

Week 3 (8/4-8/10):
  A: prctl PR_SET_NAME + set_tid_address + getpriority/setpriority
  B: symlinkat/readlinkat 验证 + pipe2 O_CLOEXEC + ioctl SIOCGIF*
  C: sendfile socket 旁路 + mremap MAP_FIXED + poll 验证

Week 4 (8/11-8/17):
  全员集成测试 + CAgent 全量跑分 + BuildStorm 全量跑分
```

---

## 风险提醒

1. **futex** 是 BuildStorm 多线程的最大风险点 — `FUTEX_WAIT_PRIVATE`/`FUTEX_WAKE_PRIVATE` 和 bitset 必须正确，否则 pthread 同步失效导致死锁
2. **procfs 缺失严重** — CAgent 10 项中 5 项直接依赖 procfs 文件读取
3. **mmap MAP_GROWSDOWN** — Rust 编译器线程栈依赖此 flag，不支持则线程创建失败
4. **epoll 缺失** — cargo/rustc 可能在内部用 tokio/mio，epoll 不可用会 fallback 到 ppoll，但某些异步路径可能异常
5. **时间片/调度策略** — BuildStorm 编译是 CPU 密集型，当前 500ms 时间片会导致交互延迟，建议缩短到 100ms
