# task syscall

[返回 impl-kernel](../../../README.md) · [任务系统](../../../../../../../wateros-task/readme.md)

本目录负责把 Linux 进程/线程 ABI 映射到 `wateros-task` 的 PCB、TCB、scheduler
和各资源侧表。创建流程采用“先创建未发布 task，初始化全部资源，再入队”，防止
SMP 下子任务提前运行。

## 文件与职责

| 文件 | 作用 |
| --- | --- |
| `clone.rs` / `vfork.rs` | fork、clone、clone3、vfork 创建和失败回滚。 |
| `execve.rs` | ELF 替换、线程组收敛、CLOEXEC 和地址空间切换。 |
| `task.rs` / `wait.rs` | exit、wait4/waitid、资源回收与 SIGCHLD。 |
| `process.rs` | PID/TID、进程组、会话和 set_tid_address。 |
| `sched.rs` / `priority.rs` / `ioprio.rs` | policy、affinity、nice、I/O priority。 |
| `rlimit.rs` | rlimit/umask 读写及目标进程权限。 |
| `pidfd.rs` | pidfd open/signal/getfd、poll 以及 waitid(P_PIDFD)。 |
| `personality.rs` / `unshare.rs` / `rseq.rs` | 执行域、命名空间探测和运行时兼容。 |

## 当前能力

- fork/clone/clone3、线程 TLS/TID/clear-child-tid、vfork 父等待、execve 和多线程 exec。
- exit/exit_group、wait4/waitid，支持 exited/stopped/continued、WNOWAIT 和 P_PIDFD。
- PID/TID、PGID/SID、job-control 相关查询和修改。
- 五类调度策略、参数/属性、affinity、getcpu、RR interval、nice 与 I/O priority。
- rlimit、umask、personality、prctl 常用项及 parent-death signal。
- pidfd_open、pidfd_send_signal(NULL siginfo)、pidfd_getfd、退出 poll。

## 生命周期与锁

```text
clone/fork
  -> 创建未入队 TCB/PCB
  -> 初始化 signal、cred、fd、cwd、mount namespace、SHM 等侧表
  -> start_* 发布 Ready，必要时定向 IPI

exit
  -> 记录 PCB/TCB 退出状态并唤醒 wait/pidfd 观察者
  -> 锁外释放 fd、signal、futex、SHM、MM 等资源
  -> 父进程 wait 后 reap PCB 与调度实体
```

scheduler lock、process registry lock 与任何 VFS/MM/IPC 锁不能嵌套到调度或用户
复制。失败路径必须撤销已建立的每个侧表，不能留下“存在但永远不入队”的子任务。

## 已知边界

- `CLONE_PIDFD` 尚未接入 clone 创建事务；独立 pidfd syscall 已完整可用。
- pidfd_send_signal 的非空 siginfo 尚无 queued payload 语义，返回 `EOPNOTSUPP`。
- rseq 保持 `ENOSYS`：完整实现必须在每次迁移/抢占时更新用户 rseq area 并处理 abort IP。
- setns 和完整 namespace 尚无 namespace fd/引用生命周期。
- 部分 rlimit（AS、MEMLOCK、NPROC）仍需在每一条资源分配路径统一计费。

## 扩展方向

优先把 `CLONE_PIDFD` 纳入 fork 的可回滚 fd 安装事务；随后实现 rseq 上下文切换
hook、namespace fd/setns、精确 rusage 和 capability 驱动的跨进程权限。所有新增
功能都应同时覆盖 fork/clone/exec/exit/reap 五个生命周期节点。
