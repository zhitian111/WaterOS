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
| `close` | 已接入 | 关闭动态 fd。 |
| `fstat` | 部分接入 | 128B `kstat` 布局；动态 fd 与 stdio 元数据；依赖 open 成功。 |
| `lseek` | 部分接入 | 支持普通 VFS handle；pipe 等返回 `ESPIPE`。 |
| `dup` | 待实现 | 需要复制 fd handle，并维护引用/关闭语义。 |
| `dup2`/`dup3` | 待实现 | `basic` 的 `dup2` wrapper 使用 `dup3(old, new, 0)`。 |
| `pipe`/`pipe2` | 部分接入 | 已创建 pipe fd 对；需要和 `fork` 后 fd 继承联调。 |
| `getdents64` | 待实现 | `getdents` 测例依赖目录枚举和 `linux_dirent64` 布局。 |
| `mkdir`/`mkdirat` | 部分接入 | `mkdirat` 经 `vfs::mkdir_at_current`；仅 `AT_FDCWD`；无 umask。 |
| `unlink`/`unlinkat` | 待实现 | 需要删除目录项和打开文件生命周期语义。 |
| `mount` | 待实现 | basic 期可先支持赛题所需最小挂载/伪成功策略。 |
| `umount`/`umount2` | 待实现 | 与 `mount` 成对。 |
| `brk` | 部分接入 | 有 Sv39 用户地址空间路径；无句柄时仍有假顶 fallback。 |
| `mmap` | 部分接入 | 支持匿名/文件映射骨架；共享写回、权限边界仍需补强。 |
| `munmap` | 部分接入 | 走 MM `MmapOps`。 |
| `mprotect` | 部分接入 | libc/动态链接后续会继续依赖。 |
| `clone`/`fork` | 待实现 | basic 的 `fork()` 实际调用 `clone(SIGCHLD, 0)`。 |
| `execve` | 待实现 | 需要替换当前用户地址空间、argv/envp 拷贝和 fd 继承规则。 |
| `exit`/`exit_group` | 已接入 | 目前同一路径退出当前任务。 |
| `wait`/`wait4` | 部分接入 | 支持最小父子等待和 `WNOHANG`。 |
| `sched_yield` | 已接入 | 映射到 `task::yield_now()`。 |
| `getpid` | 已接入 | 返回当前 task id。 |
| `getppid` | 部分接入 | orphan parent 暂返回 1。 |
| `gettid` | 部分接入 | 当前默认等同 `getpid`。 |
| `gettimeofday` | 部分接入 | 基于调度 tick 的临时时间。 |
| `clock_gettime` | 部分接入 | 基于调度 tick 的临时时间。 |
| `nanosleep` | 部分接入 | 非零睡眠临时映射为 1 个 tick。 |
| `times` | 部分接入 | 返回当前 tick 和最小 `tms`。 |
| `getcwd` | 部分接入 | 依赖 per-task cwd 注册。 |
| `chdir` | 已接入 | 经 `vfs::cwd::chdir_current`；目标须为已存在目录。 |
| `uname` | 待实现 | 需要填充 Linux `utsname` 结构。 |

## busybox/libc/benchmark 后续常见项

| syscall 能力 | 当前状态 | 说明 |
| --- | --- | --- |
| `ioctl` | 待实现 | TTY、设备、网络工具会大量触发。 |
| `fcntl` | 待实现 | fd flags、`F_DUPFD_CLOEXEC`、非阻塞等。 |
| `prctl` | 待实现 | libc/线程运行时常见探测项，可先兼容常用 no-op。 |
| `futex` | 待实现 | pthread、动态链接、benchmark 的关键同步原语。 |
| `rt_sigaction` | 待实现 | busybox、lmbench、cyclictest 需要信号安装。 |
| `rt_sigprocmask` | 待实现 | 与 signal/pthread 联动。 |
| `rt_sigreturn` | 待实现 | 完整用户信号返回路径。 |
| `set_tid_address` | 部分接入 | 当前返回 tid；未实现 clear-child-tid 唤醒。 |
| `set_robust_list` | 待实现 | pthread robust futex 兼容项。 |
| `getrandom` | 待实现 | libc 初始化和测试程序可能探测。 |
| `setitimer` | 待实现 | signal/timer 相关测例会用。 |
| `getrlimit` | 待实现 | shell/libc 常见探测项。 |
| `setrlimit` | 待实现 | 可先支持最小 no-op/参数校验。 |

## 维护规则

1. 新增 syscall 号时，先同步 `wateros-abi` 的 `SyscallNumberTable`。
2. 新增可分发能力时，同步 `wateros-syscall-api-v0::SyscallKind` 和
   `SyscallDispatcher`。
3. 真正落地实现后，在 `syscall-impl/impl-kernel/src/sys/` 增加或扩展对应
   `sys_*`，并把本文档状态从“待实现”更新为“部分接入”或“已接入”。
