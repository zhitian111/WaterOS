# WaterOS syscall TODO

本文档跟踪 `test_case` 用户态测例需要的 syscall 能力，并和
`syscall-api/api-v0::SyscallDispatcher` 的分发槽位保持同步。

## 状态口径

- **已接入**：`KernelSyscallDispatcher` 已路由到具体 `sys_*`，可用于当前 bring-up。
- **部分接入**：已有 handler，但语义是最小实现或有明显限制。
- **待实现**：API trait 已保留分发槽位，目前默认返回 `ENOSYS`。

## basic/run-all.sh 相关

| syscall 能力 | 当前状态 | 说明 |
| --- | --- | --- |
| `read` | 部分接入 | 走 per-task fd/VFS handle；stdin 真实输入未接。 |
| `write` | 部分接入 | 走 per-task fd/VFS handle；stdout/stderr 依赖 VFS 标准 fd。 |
| `open`/`openat` | 部分接入 | `AT_FDCWD` + 目录 fd 相对路径、`O_DIRECTORY`；bring-up 任务 cwd 设为 ELF 父目录。 |
| `faccessat` (48) | 部分接入 | Linux 三参数 ABI：`flags` 恒为 0、忽略 a3；经 VFS `metadata` + mode 位检查。 |
| `faccessat2` (439) | 部分接入 | `dispatch_unknown` 旁路；支持 `AT_EACCESS`/`AT_EMPTY_PATH`；`AT_SYMLINK_NOFOLLOW` 仅校验（VFS 尚无 nofollow 元数据路径）。 |
| `close` | 已接入 | 关闭动态 fd。 |
| `fstat` | 部分接入 | 128B `kstat` 布局；动态 fd 与 stdio 元数据；依赖 open 成功。 |
| `lseek` | 部分接入 | 支持普通 VFS handle；pipe 等返回 `ESPIPE`。 |
| `dup` | 已接入 | `vfs::fd::dup_fd`。 |
| `dup2`/`dup3` | 已接入 | `dup3`；`dup2` wrapper 使用 `dup3(old, new, 0)`。 |
| `pipe`/`pipe2` | 部分接入 | pipe fd 对；fork 经 `copy_fd_table_from_parent` 继承。 |
| `getdents64` | 部分接入 | `linux_dirent64`；目录 open 须 `O_DIRECTORY`（否则 `EISDIR`）。 |
| `mkdir`/`mkdirat` | 部分接入 | `mkdirat` 仅 `AT_FDCWD`；RO 辅助卷返回 `EROFS`。 |
| `unlink`/`unlinkat` | 部分接入 | `unlinkat` 经 VFS；RO 辅助卷 `EROFS`。 |
| `renameat2` | 部分接入 | `flags=0` 同父目录 rename（文件与目录），经 VFS/FS `rename` + ext4 link/unlink；跨目录、覆盖目标、`RENAME_*` flags 未实现。 |
| `utimensat` | 部分接入 | bring-up stub：校验路径/flags/timespec 后成功返回，暂不持久化 atime/mtime；后续需扩展 VFS/FS 时间戳接口并落到 ext4。 |
| `mount` | 部分接入 | `MS_RDONLY` → `mount_aux_ro` + 辅助 RO 表；否则 RW。 |
| `umount`/`umount2` | 部分接入 | `vfs::unmount_at`。 |
| `brk` | 部分接入 | 有 Sv39 用户地址空间路径；无句柄时仍有假顶 fallback。 |
| `mmap` | 部分接入 | 支持匿名/文件映射骨架；共享写回、权限边界仍需补强。 |
| `munmap` | 部分接入 | 走 MM `MmapOps`。 |
| `mprotect` | 部分接入 | libc/动态链接后续会继续依赖。 |
| `clone`/`fork` | 部分接入 | `fork_user_aspace` + 子进程保留父 fork 点 `user_sp`；继承 cwd/fd。 |
| `execve` | 部分接入 | 替换地址空间/入口/栈；CLOEXEC 等待关闭未完整。 |
| `exit`/`exit_group` | 已接入 | 目前同一路径退出当前任务。 |
| `wait`/`wait4` | 部分接入 | 支持最小父子等待和 `WNOHANG`。 |
| `sched_yield` | 已接入 | 映射到 `task::yield_now()`。 |
| `getpid` | 已接入 | 返回当前 task id。 |
| `getppid` | 部分接入 | orphan parent 暂返回 1。 |
| `gettid` | 部分接入 | 当前默认等同 `getpid`。 |
| `getuid`/`geteuid`/`getgid`/`getegid` | 已接入 | 经 `wateros-cred` 读取当前任务凭证。 |
| `getgroups` | 已接入 | G1：`[0]`；未实现边界 panic。 |
| `setuid`/`setgid`/`setreuid`/`setregid`/`setresuid`/`setresgid` | 已接入 | impl-root 允许 ID 更新；非法超宽 uid/gid 返回 `EINVAL`。 |
| `gettimeofday` | 部分接入 | REALTIME，基于 `platform::timer` + 可调 offset。 |
| `clock_settime` | 部分接入 | 仅 `CLOCK_REALTIME`；无 `CAP_SYS_TIME` 校验。 |
| `clock_gettime` | 部分接入 | REALTIME/MONOTONIC/CPUTIME 等子集；`platform::timer` 优先。 |
| `clock_getres` | 部分接入 | 返回 timer Hz 或调度 tick 分辨率。 |
| `clock_nanosleep` | 部分接入 | 相对/绝对 sleep；精度受 10ms 调度 tick 约束。 |
| `nanosleep` | 部分接入 | 与 `clock_nanosleep` 共享 sleep 换算。 |
| `times` | 部分接入 | 返回当前 tick 和最小 `tms`。 |
| `getrusage` | 部分接入 | 返回 Linux 64-bit `struct rusage`；SELF/THREAD 仅填最小 user tick，children 与资源计数暂为 0。 |
| `getcwd` | 部分接入 | 依赖 per-task cwd 注册。 |
| `chdir` | 已接入 | 经 `vfs::cwd::chdir_current`；目标须为已存在目录。 |
| `uname` | 部分接入 | 固定 WaterOS `utsname` 字段。 |
| `pread64` / `pwrite64` | 已接入 | `sys/posix_at_io.rs`；`vfs_io_at_error_to_errno` |
| `preadv` / `pwritev` | 已接入 | 与 `writev` 同 iovec 布局；上限 4MiB |
| `sendfile` | 部分接入 | `sys/sendfile.rs`；文件 in → 文件/socket out；无零拷贝 |
| `ppoll` (73) | 已接入 | `poll_engine` + `sys/poll_multiplex.rs` |
| `pselect6` (72) | 已接入 | `fd_set` + 共享引擎 |
| `select` (23) | 已接入 | `timeval` → timespec；不写回剩余超时 |
| `poll` (271) | 已接入 | 毫秒 timeout；委托 `poll_engine` |

## busybox/libc/benchmark 后续常见项

| syscall 能力 | 当前状态 | 说明 |
| --- | --- | --- |
| `ioctl` | 待实现 | TTY、设备、网络工具会大量触发。 |
| `fcntl` | 待实现 | fd flags、`F_DUPFD_CLOEXEC`、非阻塞等。 |
| `prctl` | 待实现 | libc/线程运行时常见探测项，可先兼容常用 no-op。 |
| `futex` | 已接入 | WAIT/WAKE（含 bitset）；委托 `ipc-futex/impl-task`；带超时；非 private → `EINVAL`。 |
| `rt_sigaction` | 待实现 | busybox、lmbench、cyclictest 需要信号安装。 |
| `rt_sigprocmask` | 待实现 | 与 signal/pthread 联动。 |
| `rt_sigreturn` | 待实现 | 完整用户信号返回路径。 |
| `set_tid_address` | 部分接入 | 当前返回 tid；未实现 clear-child-tid 唤醒。 |
| `set_robust_list` | 已接入 | 校验头长 24；状态存 `FutexHub`；退出深清理。 |
| `get_robust_list` | 已接入 | ABI 号 100；读回 per-task robust 头。 |
| `getrandom` | 待实现 | libc 初始化和测试程序可能探测。 |
| `setitimer` | 部分接入 | 校验 `itimerval` 并写回零值 old timer；暂不维护真实 interval timer / SIGALRM。 |
| `getrlimit` | 待实现 | shell/libc 常见探测项。 |
| `setrlimit` | 待实现 | 可先支持最小 no-op/参数校验。 |

## 维护规则

1. 新增 syscall 号时，先同步 `wateros-abi` 的 `SyscallNumberTable`。
2. 新增可分发能力时，同步 `wateros-syscall-api-v0::SyscallKind` 和
   `SyscallDispatcher`。
3. 真正落地实现后，在 `syscall-impl/impl-kernel/src/sys/` 增加或扩展对应
   `sys_*`，并把本文档状态从“待实现”更新为“部分接入”或“已接入”。
