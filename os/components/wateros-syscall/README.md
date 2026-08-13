# wateros-syscall

[项目首页](../../../README.md) · [内核工程](../../README.md) · [系统架构](../../../README.md#系统架构)

`wateros-syscall` 统一维护用户态、trap 层和内核 handler 之间的 syscall 契约与实现。
原 `wateros-abi` 只包含 syscall 相关类型，现已并入 `syscall-api/api-v0`，避免调用号
和参数/返回编码分散在两个组件中。

## 模块分层


| 层          | 路径                        | 职责                                                                                    |
| ------------- | ----------------------------- | ----------------------------------------------------------------------------------------- |
| 聚合门面    | `src/lib.rs`                | 重导出 API 与内核入口：trap 分发、信号投递/恢复、进程/线程退出、可重启 syscall 判断。   |
| syscall API | `syscall-api/api-v0/`       | 调用号、参数、errno、返回值契约；纯`no_std` 数据契约，不依赖 platform/task/MM。         |
| 内核实现    | `syscall-impl/impl-kernel/` | 单次 match 分发器、各`sys_*` 具体语义、用户内存安全拷贝、socket/poll/epoll 与 AF_UNIX。 |

## 实现说明

- 关键编码约定：


| 类型              | 内核内部表示         | 跨用户态边界时的表示                   |
| ------------------- | ---------------------- | ---------------------------------------- |
| `ErrNo`           | 正数 Linux errno     | 不能直接作为 syscall 返回值            |
| `KernelResult<T>` | `Result<T, ErrNo>`   | 尚未编码                               |
| `UserRet`         | 单个`isize`          | 成功为非负值，错误为`-errno`           |
| `SyscallArgs`     | 固定数量`usize` 槽位 | 槽位顺序必须与目标架构的参数寄存器一致 |
| `SyscallNumber`   | 裸调用号的透明包装   | 不保证该调用号已由内核实现             |

- handler 应在最终返回到 trap 层前使用 `UserRet::from_success` / `from_error` /
  `from_kernel_result`；不要在中间层把 `ErrNo` 预先取负，否则容易发生双重编码。
- 分发采用调用号单次 `match`（H-3），替代旧的 `SyscallKind::decode` + 巨型 match；未命中的
  调用号走旁路号与 `ENOSYS`。
- 用户内存访问集中在 `user_copy.rs`：`copy_from_user` / `copy_to_user` 经 `ActiveUserMemoryOps`，
  空缓冲返回 0、指针 0 返回 `EFAULT`；`USER_PATH_MAX = 4096`，可用 `user-copy-diagnostics`
  记录失败。
- `is_restartable_syscall` 供 trap 层在 `EINTR` 后判断是否自动重启该 syscall。
- 依赖方向：`syscall-api-v0` 是纯数据契约，禁止反向依赖 platform/task/MM 等组件；`impl-kernel`
  可依赖这些服务并实现具体语义。
- 边界划分：调用号、errno、参数和返回编码在 `syscall-api-v0`；trap 帧与参数寄存器读写由
  `wateros-platform-arch-api-v0` 和 arch impl；业务实现、用户内存访问与子系统错误映射在
  `impl-kernel`。

## 调用链路

trap 进入：

```text
trap / 异常返回路径
  -> dispatch_syscall_from_trap(nr, args)
  -> sys::record_syscall()（统计）
  -> dispatch_syscall_by_nr(nr, args)（单次 match）
  -> sys_* handler -> UserRet 编码 -> isize
```

信号与退出：

```text
信号投递：deliver_pending_signal(frame, restart) -> restore_signal_frame(frame)
进程退出：terminate_current_process(exit_code) -> sys_exit_group（SMP 退出流程）
线程退出：terminate_current_thread(exit_code) -> exit_current_with_wait_code
```

## 各实现功能

### syscall-api / API 契约

主要实现在 `syscall-api/api-v0/src/`。

- 定义 Linux generic 64 位调用号常量与 `SyscallNumber` 透明包装，供 trap 层与分发器共用。
- 定义参数传递：`SyscallArgs` 固定 `usize` 槽位（数量取 `config::syscall::MAX_SYSCALL_ARGS`），
  `from_regs` / `as_regs` 保证槽位顺序与目标架构寄存器一致；`SyscallPacket` 打包调用号与参数。
- 定义错误与返回编码：`ErrNo` 为正数 Linux errno，`KernelResult<T>` 未编码，`UserRet` 提供
  `from_success` / `from_error` / `from_kernel_result`，保证错误只编码一次。

### impl-kernel / 内核实现

主要实现在 `syscall-impl/impl-kernel/src/`。

- 分发：`syscall_nr_dispatch.rs` 按裸调用号单次 match 路由到各 `sys_*`，未命中走旁路号与
  `ENOSYS`；`is_restartable_syscall` 供 EINTR 自动重启。
- 用户内存安全拷贝：`user_copy.rs` 经 `ActiveUserMemoryOps` 提供 `copy_from_user` /
  `copy_to_user` / 字符串拷贝，指针 0 → `EFAULT`，可记录失败诊断。
- 文件系统类（`sys/fs`）：open/close/read/write/readv/pwrite、lseek、fstat/statfs/getdents64、
  fcntl/flock/dup/dup3、pipe2/eventfd2、mkdirat/renameat2/truncate/fallocate、sendfile、
  xattr/attr、ioctl 等。
- 进程与线程类（`sys/task`，详见其 `readme.md`）：clone/vfork/execve、exit/exit_group/wait、
  sched/priority/rlimit、unshare/rseq、进程组与信号投递。
- IPC 与信号类（`sys/ipc`）：futex（含 requeue/robust）、eventfd、SysV shm、signal、kill 等。
- 网络与 socket 类（`sys/net` + `socket_fd.rs` / `socket_block.rs` / `unix_sock.rs`）：
  socket/socketpair/bind/listen/accept/connect、sendto/recvfrom/sendmsg/shutdown、sockname/
  sockopt；AF_UNIX 流 socket 的 accept 队列与 poll 语义。
- 内存、时间与多路复用类：`sys/mem`（mmap/brk/mempolicy）、`sys/time`（clock/timer/rtc/
  posix_timer）、`sys/poll`（poll/select/epoll，含 `poll_engine.rs` / `epoll_fd.rs`）。
- 凭证类（`sys/cred`）：uid/gid 相关（委托 `wateros-cred`）。
- 其它（`sys/misc`）：syslog、mount/umount2、sysinfo、acct、sync、riscv_hwprobe、
  bringup_stats。
- 辅助模块：`fallible_buf.rs`（可失败缓冲写入）、`linux_stat.rs` / `stat_times.rs`（stat ABI
  与时间戳）、`mm_util.rs` / `vfs_util.rs`（mm/VFS 互操作）、`socket_fd.rs`（fd 适配）。
