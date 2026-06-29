# wateros-syscall — 已实现功能快照

## 用途

记录 `wateros-syscall` 一级组件当前已落地能力、feature 组合与已知缺口。事实来源：`os/components/wateros-syscall/**` 源码与 `Cargo.toml`。

## 子 crate 与职责

| 子 crate | 职责 | 状态 |
|----------|------|------|
| `wateros-syscall`（聚合） | trap/C ABI 入口、`active_impl` 门面、`api` 再导出 | 已实现 |
| `syscall-api/api-v0` | `SyscallKind`、`SyscallDispatcher` trait、未实现槽位辅助 | 已实现 |
| `syscall-impl/impl-kernel` | `sys_*` 实现、`KernelSyscallDispatcher`、号表单次 match 分发 | 已实现 |

## Feature 矩阵（聚合层）

| Feature | 效果 |
|---------|------|
| `default` | 启用 `impl-kernel`（含 `api-v0`） |
| `api-v0` | 导出 `syscall::api::*` 与 `SyscallDispatcher` trait |
| `impl-kernel` | 链接内核实现；导出 trap 分发与信号/定时器 hook |

## 已实现能力（按域）

- **分发**：`dispatch_syscall_by_nr` 裸号 O(1) match；旁路号（`statx`、`fstatat`、`faccessat2`、`close_range`、`setgroups` 等）与 xattr 裸号 5–16。
- **文件 I/O**：`read`/`write`/`pread*`/`pwrite*`/`readv`/`writev`、`openat`、`close`、`dup`/`dup3`、`pipe2`、`lseek`、`sendfile`、`fcntl`、`flock`、`ioctl`（含 RTC/TTY 桩）。
- **路径/VFS**：`getcwd`/`chdir`、`*at` 族（`mkdirat`、`unlinkat`、`renameat2`、`symlinkat`、`readlinkat`、`utimensat`）、`getdents64`、`mount`/`umount2`、`faccessat`/`faccessat2`、`fchmodat`/`fchownat`、xattr 全族、`stat`/`fstat`/`statx`/`statfs`、截断与 `fallocate`。
- **内存**：`brk`、`mmap` 族、`mprotect`、`mremap`、`madvise`、`mlock*`、`get_mempolicy`、SysV shm 子集。
- **进程/线程**：`clone`/`clone3`、`execve`、`exit`/`exit_group`、`waitpid`/`waitid`、`getpid` 族、`sched_*`、`setpgid`/`getpgid`、`priority`、`unshare`（`CLONE_NEWNS`）、robust futex 列表。
- **凭证**：`getuid`/`setuid` 等经 `wateros-cred`；`capget`/`capset` 最小桩。
- **信号/同步**：`futex`、`rt_sig*`、`kill`/`tkill`/`tgkill`、`nanosleep`/`clock_nanosleep`、EINTR 可重启 syscall 跳表。
- **多路复用**：`poll`/`ppoll`/`select`/`pselect6`、`epoll_*`。
- **网络**：INET socket 全族 + `AF_UNIX` pathname/abstract；smoltcp 后端。
- **时间**：`clock_*`、`gettimeofday`、`adjtimex`/`clock_adjtime` 桩。
- **杂项**：`syslog`→klog、`sysinfo`、`uname`、`prctl` 子集、`getrandom`、`acct`/`close_range` 兼容、LTP fast-exit 协作模块。

## 与 trap 的接线

- `dispatch_syscall_from_trap` / `__wateros_syscall_dispatch_current`
- `timer_tick`、`deliver_pending_signal`、`restore_signal_frame`、`is_restartable_syscall`
- 任务 reap：`drop_reaped_task_runtime_resources`

## 缺口与后续

- trait 默认 `dispatch_*` 未覆盖的槽位在 dummy impl 返回 `-ENOSYS`；`impl-kernel` 对未实现槽位 **panic**（bring-up 策略）。
- `utimensat`、部分 `ioctl`/控制终端、真实权限校验仍是最小语义。
- LoongArch 与 RISC-V 共用 generic 64-bit 号表；架构差异在 platform trap 层。

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出（注释/inline 任务同步） |
