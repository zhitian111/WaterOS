# 系统调用清单（盘点稿）

> **生成阶段**：审计任务第 1 步 — 拆分子任务前供确认。  
> **事实来源**：`wateros-abi/impl-linux-generic64`、`syscall-api/api-v0::SyscallKind`、`syscall-impl/impl-kernel::KernelSyscallDispatcher`、`os/src/trap_handler.rs`。  
> **ABI 基线**：Linux asm-generic 64 位号表（RISC-V 64 / LoongArch 64 共用 `ActiveSyscallNumberTable`）。

## 统计摘要

| 项 | 数量 |
|----|------|
| ABI 号表已注册（`SyscallNumberTable`） | **139** 常量（**138** 可解码 nr + `SELECT` 哨兵 `usize::MAX`） |
| `dispatch_unknown` 旁路（号表未收录但已实现） | **8** |
| trap 特殊路径（不经 `KernelSyscallDispatcher`） | **1**（`rt_sigreturn`） |
| **`sys_*` 实现** | **146**（含号表不可达的 `sys_select`） |
| **合计需审计的路由目标** | **138 槽位 + 8 旁路 nr**（`rt_sigreturn` 含于 138） |
| 建议 subagent 合并组 | **~52**（见文末拆分计划） |

## 路由说明

1. **主路径**：`trap_handler` → `dispatch_syscall_from_trap` → `SyscallKind::decode` → `KernelSyscallDispatcher::dispatch_*` → `sys_*`。
2. **`dispatch_unknown`**：解码为 `SyscallKind::Unknown(nr)` 时，内核对 8 个 nr 做旁路，其余返回 `-ENOSYS`。
3. **`rt_sigreturn`(139)**：在 `trap_handler.rs` 中于分发前拦截，调用 `restore_signal_frame`，**不进入** `KernelSyscallDispatcher`。
4. **`select` 号表缺口**：`SELECT = usize::MAX`（哨兵），`sys_select` 已实现但**无法通过号表解码到达**；riscv64 用户态应走 `pselect6`(72)。若误用 nr **23** 调用 select，会命中 **`dup`(23)** — 属已知语义陷阱。
5. **未覆盖槽位默认行为**：`KernelSyscallDispatcher` 对 trait 默认槽位调用 `dispatch_unsupported` → **内核 panic**（非 `ENOSYS`）。当前除 `rt_sigreturn` 外均已 override。

## 复杂度图例

| 标记 | 含义 |
|------|------|
| **L** | 薄封装 / 单文件 / 语义简单 |
| **M** | 多分支、VFS/MM/IPC 下游、部分 flag 未覆盖 |
| **H** | 进程/线程/信号/多路复用/网络/地址空间，卡死或语义偏差风险高 |

## 初步状态图例

| 标记 | 含义 |
|------|------|
| 已接入 | 主路径可用，与 Linux 大体一致 |
| 部分 | 最小实现或文档/TODO 已标明限制 |
| stub | 校验后成功返回或 no-op，不持久化真实语义 |
| 旁路 | 仅 `dispatch_unknown` 可达 |

---

## 一、I/O 与文件描述符

| 名称 | nr | 分发入口 | 实现文件 | 复杂度 | 初步状态 |
|------|-----|----------|----------|--------|----------|
| read | 63 | dispatch_read | sys/read.rs | H | 部分（stdin 无真实输入；pipe/socket 可阻塞） |
| readv | 65 | dispatch_readv | sys/read.rs | H | 部分 |
| write | 64 | dispatch_write | sys/write.rs | M | 部分 |
| writev | 66 | dispatch_writev | sys/write.rs | M | 部分 |
| pread64 | 67 | dispatch_pread64 | sys/posix_at_io.rs | M | 已接入 |
| pwrite64 | 68 | dispatch_pwrite64 | sys/posix_at_io.rs | M | 已接入 |
| preadv | 69 | dispatch_preadv | sys/posix_at_io.rs | M | 已接入 |
| pwritev | 70 | dispatch_pwritev | sys/posix_at_io.rs | M | 已接入 |
| sendfile | 71 | dispatch_sendfile | sys/sendfile.rs | M | 部分 |
| dup | 23 | dispatch_dup | sys/dup.rs | L | 已接入 |
| dup3 | 24 | dispatch_dup3 | sys/dup.rs | L | 已接入 |
| pipe2 | 59 | dispatch_pipe2 | sys/pipe2.rs | M | 部分 |
| close | 57 | dispatch_close | sys/close.rs | L | 已接入 |
| ioctl | 29 | dispatch_ioctl | sys/ioctl.rs | H | 部分（TTY/RTC 子集） |
| fcntl | 25 | dispatch_fcntl | sys/fcntl.rs | M | 部分（常见 cmd；其余 ENOSYS） |

## 二、路径与 VFS

| 名称 | nr | 分发入口 | 实现文件 | 复杂度 | 初步状态 |
|------|-----|----------|----------|--------|----------|
| openat | 56 | dispatch_openat | sys/openat.rs | H | 部分（flag/相对路径子集） |
| faccessat | 48 | dispatch_faccessat | sys/faccessat.rs | M | 部分（三参 ABI，忽略 a3） |
| faccessat2 | 439 | dispatch_unknown | sys/faccessat.rs | M | 部分（旁路） |
| fchmodat | 53 | dispatch_fchmodat | sys/fchmodat.rs | M | 部分 |
| fchownat | 54 | dispatch_fchownat | sys/fchownat.rs | M | 部分 |
| readlinkat | 78 | dispatch_readlinkat | sys/readlinkat.rs | M | 部分 |
| statfs | 43 | dispatch_statfs | sys/statfs.rs | M | 部分 |
| fstat | 80 | dispatch_fstat | sys/fstat.rs | M | 部分（128B kstat） |
| fstatat | 79 | dispatch_unknown | sys/fstat.rs | M | 部分（旁路） |
| statx | 291 | dispatch_unknown | sys/fstat.rs | M | 部分（旁路） |
| lseek | 62 | dispatch_lseek | sys/lseek.rs | M | 部分（pipe→ESPIPE） |
| getdents64 | 61 | dispatch_getdents64 | sys/getdents64.rs | M | 部分（须 O_DIRECTORY） |
| mkdirat | 34 | dispatch_mkdirat | sys/mkdirat.rs | M | 部分（仅 AT_FDCWD） |
| symlinkat | 36 | dispatch_symlinkat | sys/symlinkat.rs | M | 部分 |
| unlinkat | 35 | dispatch_unlinkat | sys/unlinkat.rs | M | 部分 |
| renameat2 | 276 | dispatch_renameat2 | sys/renameat2.rs | M | 部分（flags=0 同目录） |
| utimensat | 88 | dispatch_utimensat | sys/utimensat.rs | L | stub（不持久化时间戳） |
| sync | 81 | dispatch_sync | sys/sync.rs | L | 部分 |
| fsync | 82 | dispatch_fsync | sys/sync.rs | L | 部分 |
| fdatasync | 83 | dispatch_fdatasync | sys/sync.rs | L | 部分 |
| ftruncate | 46 | dispatch_ftruncate | sys/ftruncate.rs | M | 部分 |
| fallocate | 47 | dispatch_fallocate | sys/fallocate.rs | M | 部分 |
| mount | 40 | dispatch_mount | sys/mount.rs | H | 部分（MS_RDONLY 等子集） |
| umount2 | 39 | dispatch_umount2 | sys/umount2.rs | M | 部分 |
| getcwd | 17 | dispatch_getcwd | sys/getcwd.rs | M | 部分 |
| chdir | 49 | dispatch_chdir | sys/chdir.rs | L | 已接入 |

## 三、进程、执行与调度

| 名称 | nr | 分发入口 | 实现文件 | 复杂度 | 初步状态 |
|------|-----|----------|----------|--------|----------|
| exit | 93 | dispatch_exit | sys/task.rs | L | 已接入 |
| exit_group | 94 | dispatch_exit_group | sys/task.rs | L | 已接入 |
| clone / fork | 220 | dispatch_clone | sys/clone.rs | H | 部分（flag 子集；线程路径） |
| clone3 | 435 | dispatch_clone3 | sys/clone.rs | H | 部分 |
| execve | 221 | dispatch_execve | sys/execve.rs | H | 部分（shebang 无 PATH 搜索） |
| waitpid / wait4 | 260 | dispatch_waitpid | sys/task.rs | H | 部分（可阻塞；WNOHANG） |
| kill | 129 | dispatch_kill | sys/kill.rs | M | 部分 |
| sched_yield | 124 | dispatch_yield | sys/task.rs | L | 已接入 |
| sched_setparam | 118 | dispatch_sched_setparam | sys/sched.rs | M | 已接入 |
| sched_setscheduler | 119 | dispatch_sched_setscheduler | sys/sched.rs | M | 已接入 |
| sched_getscheduler | 120 | dispatch_sched_getscheduler | sys/sched.rs | L | 已接入 |
| sched_getparam | 121 | dispatch_sched_getparam | sys/sched.rs | L | 已接入 |
| sched_setaffinity | 122 | dispatch_sched_setaffinity | sys/sched.rs | L | 部分（恒 EPERM） |
| sched_getaffinity | 123 | dispatch_sched_getaffinity | sys/sched.rs | M | 部分（单核 CPU0） |
| sched_get_priority_max | 125 | dispatch_sched_get_priority_max | sys/sched.rs | L | 已接入 |
| sched_get_priority_min | 126 | dispatch_sched_get_priority_min | sys/sched.rs | L | 已接入 |
| sched_setattr | 274 | dispatch_unknown | sys/sched.rs | M | 部分（旁路） |
| sched_getattr | 275 | dispatch_unknown | sys/sched.rs | M | 部分（旁路） |
| getpid | 172 | dispatch_getpid | sys/task.rs | L | 已接入 |
| getppid | 173 | dispatch_getppid | sys/task.rs | L | 部分（orphan→1） |
| gettid | 178 | dispatch_gettid | sys/task.rs | L | 部分（≈getpid） |
| setsid | 157 | dispatch_setsid | sys/task.rs | M | 部分 |
| setpgid | 154 | dispatch_setpgid | sys/task.rs | M | 部分 |
| set_tid_address | 96 | dispatch_set_tid_address | sys/task.rs | M | 部分（无 clear_child_tid 唤醒） |

## 四、内存管理

| 名称 | nr | 分发入口 | 实现文件 | 复杂度 | 初步状态 |
|------|-----|----------|----------|--------|----------|
| brk | 214 | dispatch_brk | sys/brk.rs | M | 部分（Sv39；无 aspace 时 fake） |
| mmap | 222 | dispatch_mmap | sys/mmap.rs | H | 部分 |
| munmap | 215 | dispatch_munmap | sys/mmap.rs | M | 部分 |
| mprotect | 226 | dispatch_mprotect | sys/mmap.rs | M | 部分 |
| mremap | 216 | dispatch_mremap | sys/mmap.rs | H | 部分 |
| madvise | 233 | dispatch_madvise | sys/mmap.rs | L | 部分 |
| msync | 227 | dispatch_msync | sys/mmap.rs | M | 部分 |
| mlock | 228 | dispatch_mlock | sys/mmap.rs | L | stub（no-op 成功） |
| munlock | 229 | dispatch_munlock | sys/mmap.rs | L | stub |
| mlockall | 230 | dispatch_mlockall | sys/mmap.rs | L | stub |
| munlockall | 231 | dispatch_munlockall | sys/mmap.rs | L | stub |
| get_mempolicy | 236 | dispatch_getmempolicy | sys/mempolicy.rs | M | 部分 |
| shmget | 194 | dispatch_shmget | sys/shm.rs | M | 部分 |
| shmctl | 195 | dispatch_shmctl | sys/shm.rs | M | 部分 |
| shmat | 196 | dispatch_shmat | sys/shm.rs | H | 部分 |
| shmdt | 197 | dispatch_shmdt | sys/shm.rs | M | 部分 |

## 五、时间与计时器

| 名称 | nr | 分发入口 | 实现文件 | 复杂度 | 初步状态 |
|------|-----|----------|----------|--------|----------|
| gettimeofday | 169 | dispatch_get_time | sys/clock.rs | M | 部分 |
| clock_settime | 112 | dispatch_clock_settime | sys/clock.rs | M | 部分 |
| clock_gettime | 113 | dispatch_clock_gettime | sys/clock.rs | M | 部分 |
| clock_getres | 114 | dispatch_clock_getres | sys/clock.rs | L | 部分 |
| clock_nanosleep | 115 | dispatch_clock_nanosleep | sys/clock.rs | H | 部分（~10ms tick） |
| nanosleep | 101 | dispatch_nanosleep | sys/clock.rs | H | 部分 |
| times | 153 | dispatch_times | sys/task.rs | L | 部分 |
| setitimer | 103 | dispatch_setitimer | sys/task.rs | M | 已接入 |
| getitimer | 102 | dispatch_getitimer | sys/task.rs | M | 已接入 |
| adjtimex | 171 | dispatch_unknown | sys/clock.rs | M | 部分（旁路） |
| clock_adjtime | 266 | dispatch_unknown | sys/clock.rs | M | 部分（旁路） |

## 六、身份、凭证与系统信息

| 名称 | nr | 分发入口 | 实现文件 | 复杂度 | 初步状态 |
|------|-----|----------|----------|--------|----------|
| getuid | 174 | dispatch_getuid | sys/cred.rs | L | 已接入 |
| geteuid | 175 | dispatch_geteuid | sys/cred.rs | L | 已接入 |
| getgid | 176 | dispatch_getgid | sys/cred.rs | L | 已接入 |
| getegid | 177 | dispatch_getegid | sys/cred.rs | L | 已接入 |
| getgroups | 158 | dispatch_getgroups | sys/cred.rs | M | 已接入（边界可 panic） |
| setuid | 146 | dispatch_setuid | sys/cred.rs | L | 已接入 |
| setgid | 144 | dispatch_setgid | sys/cred.rs | L | 已接入 |
| setreuid | 145 | dispatch_setreuid | sys/cred.rs | L | 已接入 |
| setregid | 143 | dispatch_setregid | sys/cred.rs | L | 已接入 |
| setresuid | 147 | dispatch_setresuid | sys/cred.rs | L | 已接入 |
| setresgid | 149 | dispatch_setresgid | sys/cred.rs | L | 已接入 |
| capget | 90 | dispatch_capget | sys/cap.rs | M | 部分 |
| capset | 91 | dispatch_capset | sys/cap.rs | M | 部分 |
| sysinfo | 179 | dispatch_sysinfo | sys/task.rs | M | 部分 |
| uname | 160 | dispatch_uname | sys/task.rs | L | 部分（固定字段） |
| prctl | 167 | dispatch_prctl | sys/task.rs | M | 部分（常用 no-op 子集） |
| getrlimit | 163 | dispatch_getrlimit | sys/task.rs | M | 部分 |
| setrlimit | 164 | dispatch_setrlimit | sys/task.rs | M | 部分 |
| prlimit64 | 261 | dispatch_prlimit64 | sys/task.rs | M | 部分 |
| umask | 166 | dispatch_umask | sys/task.rs | L | 部分 |
| getrusage | 165 | dispatch_getrusage | sys/task.rs | M | 部分 |
| getrandom | 278 | dispatch_getrandom | sys/task.rs | M | 部分 |
| syslog | 116 | dispatch_syslog | sys/syslog.rs | M | 已接入（非法指针可 panic） |
| acct | 89 | dispatch_unknown | sys/acct.rs | L | 部分（旁路） |

## 七、信号、futex 与同步

| 名称 | nr | 分发入口 | 实现文件 | 复杂度 | 初步状态 |
|------|-----|----------|----------|--------|----------|
| rt_sigreturn | 139 | **trap_handler** | sys/signal.rs + trap | H | 已接入（特殊路径） |
| rt_sigaction | 134 | dispatch_rt_sigaction | sys/task.rs | H | 已接入 |
| rt_sigprocmask | 135 | dispatch_rt_sigprocmask | sys/task.rs | H | 已接入 |
| rt_sigpending | 136 | dispatch_rt_sigpending | sys/signal.rs | M | 已接入 |
| rt_sigsuspend | 133 | dispatch_rt_sigsuspend | sys/signal.rs | H | 已接入（可阻塞） |
| rt_sigtimedwait | 137 | dispatch_rt_sigtimedwait | sys/task.rs | H | 已接入 |
| futex | 98 | dispatch_futex | sys/futex.rs | H | 已接入（非 private→EINVAL） |
| set_robust_list | 99 | dispatch_set_robust_list | sys/robust.rs | M | 已接入 |
| get_robust_list | 100 | dispatch_get_robust_list | sys/robust.rs | M | 已接入 |
| tkill | 130 | dispatch_tkill | sys/signal.rs | M | 已接入 |
| tgkill | 131 | dispatch_tgkill | sys/signal.rs | M | 已接入 |

## 八、Socket 与网络

| 名称 | nr | 分发入口 | 实现文件 | 复杂度 | 初步状态 |
|------|-----|----------|----------|--------|----------|
| socket | 198 | dispatch_socket | sys/socket.rs | M | 部分 |
| socketpair | 199 | dispatch_socketpair | sys/socketpair.rs | M | 部分（AF_UNIX STREAM） |
| bind | 200 | dispatch_bind | sys/bind.rs | M | 部分 |
| listen | 201 | dispatch_listen | sys/listen.rs | M | 部分 |
| accept | 202 | dispatch_accept | sys/accept.rs | H | 部分（可阻塞） |
| accept4 | 242 | dispatch_accept4 | sys/accept.rs | H | 部分 |
| connect | 203 | dispatch_connect | sys/connect.rs | H | 部分 |
| getsockname | 204 | dispatch_getsockname | sys/sockname.rs | M | 部分 |
| getpeername | 205 | dispatch_getpeername | sys/sockname.rs | M | 部分 |
| sendto | 206 | dispatch_sendto | sys/sendto.rs | M | 部分 |
| recvfrom | 207 | dispatch_recvfrom | sys/recvfrom.rs | H | 部分（可阻塞循环） |
| sendmsg | 211 | dispatch_sendmsg | sys/sendmsg.rs | M | 部分 |
| recvmsg | 212 | dispatch_recvmsg | sys/sendmsg.rs | M | 部分 |
| setsockopt | 208 | dispatch_setsockopt | sys/sockopt.rs | M | 部分 |
| getsockopt | 209 | dispatch_getsockopt | sys/sockopt.rs | M | 部分 |
| shutdown | 210 | dispatch_shutdown | sys/shutdown.rs | L | 部分 |

## 九、I/O 多路复用

| 名称 | nr | 分发入口 | 实现文件 | 复杂度 | 初步状态 |
|------|-----|----------|----------|--------|----------|
| ppoll | 73 | dispatch_ppoll | sys/poll_multiplex.rs + poll_engine.rs | H | 部分（sigmask 忽略） |
| pselect6 | 72 | dispatch_pselect6 | sys/poll_multiplex.rs + poll_engine.rs | H | 部分 |
| select | — | dispatch_select | sys/poll_multiplex.rs | H | 已实现但**号表不可达**；见路由说明 |
| poll | 271 | dispatch_poll | sys/poll.rs + poll_engine.rs | H | 部分 |

---

## 建议 subagent 拆分计划（~52 组）

| 组 ID | 合并 syscall | 理由 |
|-------|--------------|------|
| G01 | read, readv | 同文件、共享 read_fd |
| G02 | write, writev | 同文件 |
| G03 | pread64, pwrite64, preadv, pwritev | posix_at_io |
| G04 | dup, dup3 | 同文件 |
| G05 | pipe2, close | fd 生命周期 |
| G06 | ioctl | 独立复杂 |
| G07 | fcntl | 独立 |
| G08 | openat | 高优先级 |
| G09 | faccessat, faccessat2 | 同文件 |
| G10 | fchmodat, fchownat | at-path 权限 |
| G11 | readlinkat | |
| G12 | statfs | |
| G13 | fstat, fstatat, statx | 同文件 linux_stat |
| G14 | lseek, sendfile | fd 偏移 |
| G15 | getdents64 | |
| G16 | mkdirat, symlinkat, unlinkat, renameat2 | path_at 族 |
| G17 | utimensat | stub |
| G18 | sync, fsync, fdatasync, ftruncate, fallocate | 文件同步/大小 |
| G19 | mount, umount2 | |
| G20 | getcwd, chdir | |
| G21 | exit, exit_group | |
| G22 | clone, clone3 | |
| G23 | execve | |
| G24 | waitpid | |
| G25 | kill, tkill, tgkill | 信号投递 |
| G26 | sched_yield | |
| G27 | sched_setparam … sched_get_priority_min, sched_setattr, sched_getattr | sched.rs |
| G28 | getpid, getppid, gettid, setsid, setpgid, set_tid_address | task 身份 |
| G29 | brk | |
| G30 | mmap, munmap, mprotect, mremap, madvise, msync, mlock* | mmap.rs |
| G31 | get_mempolicy | |
| G32 | shmget, shmctl, shmat, shmdt | |
| G33 | gettimeofday, clock_* , nanosleep, adjtimex, clock_adjtime | clock.rs |
| G34 | times, setitimer, getitimer | |
| G35 | getuid…setresgid, getgroups | cred.rs |
| G36 | capget, capset | |
| G37 | sysinfo, uname, prctl, getrlimit, setrlimit, prlimit64, umask, getrusage, getrandom | task.rs 杂项 |
| G38 | syslog, acct | |
| G39 | rt_sigreturn + rt_sig* + signal.rs | 信号全族 |
| G40 | futex, set/get_robust_list | |
| G41 | socket, socketpair | |
| G42 | bind, listen, accept, accept4, connect | |
| G43 | getsockname, getpeername, shutdown | |
| G44 | sendto, recvfrom, sendmsg, recvmsg | |
| G45 | setsockopt, getsockopt | |
| G46 | ppoll, pselect6, select, poll | poll_engine |

未单独列出者已并入上表。确认后可按组并行派发 subagent。

---

## 高优先级收敛列表（初稿，汇总前供参考）

以下项易导致**卡死、无限等待、静默语义错误或 panic**；正式审计后写入 `syscall-issues.md`。

| 优先级 | syscall / 组合 | 风险类型 | 初判原因 | 建议收敛方向 |
|--------|----------------|----------|----------|--------------|
| P0 | `read`(0/stdin) | 卡死/语义 | stdin 无真实输入；脚本等待键盘 | 明确 EBADF 或立即 EOF + warn |
| P0 | `waitpid` 无 `WNOHANG` | 卡死 | 子进程状态/父子关系不完整时永久阻塞 | 校验可等待子进程集；无法等待时 ECHILD + warn |
| P0 | `ppoll`/`pselect6`/`poll` + 空 pipe/socket + 无限超时 | 卡死 | poll_engine 阻塞循环 | 文档化；未实现 wake 路径 warn + EINVAL/ENOTSUP |
| P0 | `futex` FUTEX_WAIT 未支持 op/flag | 卡死/语义 | 非 private、错误地址 | 入口拒绝未知 op，返回 EINVAL |
| P0 | `clone`/`clone3` 未支持 flag 组合 | 语义/卡死 | 部分 flag 静默忽略 | 未支持 flag warn + EINVAL |
| P0 | `execve` + `#!/usr/bin/env` | 语义 | 无 PATH 搜索 | 明确 ENOENT + warn |
| P0 | 用户页错误后反复 trap | 卡死 | trap 风暴（非 syscall 但常伴随错误 mmap） | 与 mm 审计联动 |
| P1 | `read`/`recvfrom` TCP/UDP 阻塞读 | 卡死 | read_fd / poll 中 spin+yield 循环 | 超时与非阻塞路径对齐 Linux |
| P1 | `accept`/`accept4` 阻塞 | 卡死 | 无连接时等待 | 非阻塞/O_NONBLOCK 语义审计 |
| P1 | `connect` 未完成握手 | 卡死/语义 | 网络栈不完整 | 拒绝不支持的地址族/协议 |
| P1 | `rt_sigsuspend` / `clock_nanosleep` | 卡死 | 信号/定时器精度 | 超时与 EINTR 路径 |
| P1 | `openat` 未支持 flag（O_TMPFILE、O_PATH 等） | 静默错误 | 部分 flag 未校验 | warn + EINVAL |
| P1 | `mmap` MAP_SHARED 写回 / 非法 prot | 语义 | 部分映射看似成功 | 拒绝未实现组合 |
| P1 | `mount` 非 MS_RDONLY 复杂 flag | 语义 | MS_BIND 等未实现 | warn + EINVAL |
| P1 | `fcntl` 未知 cmd | 错误码 | 返回 ENOSYS 但 libc 可能重试 | 统一 EINVAL/ENOSYS 策略 |
| P1 | `ioctl` 未知 request | 语义/panic | 大量设备请求 | 分 request 收敛 |
| P1 | nr **23** 误作 `select` | 语义陷阱 | 实际命中 `dup` | 文档 + 可选兼容层 |
| P2 | `getgroups` 缓冲区边界 | panic | TODO 注明边界 panic | EFAULT + warn |
| P2 | `syslog` 空指针 | panic | 显式 panic 路径 | EFAULT |
| P2 | `utimensat` | 静默成功 | stub 不更新时间 | 文档标注；可选 EOPNOTSUPP |
| P2 | `mlock*` | 静默成功 | no-op | 可保持，但 document |
| P2 | `sched_setaffinity` | 错误码 | 恒 EPERM | 与 Linux CAP 语义对齐说明 |
| P2 | `socket` 非 AF_UNIX/非 STREAM | 语义 | 返回错误不一致 | 入口拒绝 + warn |
| P2 | `renameat2` 非零 flags | 语义 | 未实现 flag 未拒绝 | warn + EINVAL |
| P2 | `faccessat2` AT_SYMLINK_NOFOLLOW | 语义 | VFS 无 nofollow | warn + EOPNOTSUPP |
| P2 | `set_tid_address` | 语义 | 无 clear_child_tid | futex wake 缺失导致 pthread 卡死 |
| P2 | `dispatch_unsupported` 缺 override | panic | 新增号表项未接 handler | 编译期/审计检查 |

---

*确认本清单后，主 agent 将按「建议 subagent 拆分计划」并行派发，并汇总为 `syscall-issues.md` 与 `syscall-coverage.md`。*
