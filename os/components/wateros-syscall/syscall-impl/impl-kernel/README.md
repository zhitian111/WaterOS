# impl-kernel

[返回 syscall 总览](../../README.md)

这是 WaterOS 的 Linux syscall 内核适配层：把原始 ABI 参数变成 task、VFS、MM、IPC、network、TTY 和 platform 操作，并将结果编码回 Linux 返回值。它不是通用组件；裸用户指针、Linux flags/ioctl 和 errno 应在这里终止。

## 从 trap 到返回寄存器

```text
arch trap 取 syscall nr + 最多 MAX_SYSCALL_ARGS 个寄存器
 -> SyscallArgs::from_regs
 -> dispatch_syscall_from_trap
      -> record_syscall
      -> dispatch_syscall_by_nr
           -> ARG_SYSCALL_TABLE 或 SPECIAL_SYSCALL_TABLE
           -> handler
           -> UserRet.0 / raw isize
      -> 成功时 account_process_io（已不持 VFS/设备锁）
 -> trap/signal 层决定投递信号、EINTR restart、写回用户寄存器
```

分发表长度为 `EPOLL_PWAIT2 + 1`，当前是 442 个槽。先查普通 `fn(SyscallArgs)->UserRet` 表，再查特殊 `fn(SyscallArgs)->isize` 表；超范围和空槽统一 ENOSYS。EXIT/EXIT_GROUP、BRK、无参数调用及 RISC-V 专用调用使用特殊 adapter。不要把 `usize::MAX` 的 POLL 等哨兵放进表。

## 公共基础设施

| 文件 | 作用 |
| --- | --- |
| `user_copy.rs` | 页表感知的用户读写和字符串复制；坏地址返回 `EFAULT`。 |
| `fallible_buf.rs` | syscall 临时缓冲的可失败分配和防御性大小上限。 |
| `vfs_util.rs` / `mm_util.rs` | VFS、MM 领域错误到 errno 的统一映射。 |
| `poll_engine.rs` / `epoll_fd.rs` | poll/select/epoll 扫描、等待和超时。 |
| `socket_fd.rs` / `socket_block.rs` / `unix_sock.rs` | socket fd、阻塞和 AF_UNIX 状态。 |
| `linux_stat.rs` / `stat_times.rs` | Linux stat ABI 与运行时间换算。 |

公共 helper 不是绕过审计的捷径：使用前仍要确认用户结构版本、alignment、长度上限和 partial semantics。临时用户长度绝不能直接 `vec![0; user_len]`；先由 `fallible_buf` 做上限与 try_reserve，避免恶意参数触发内核 heap panic。

## 子领域

- [cred](src/sys/cred/README.md)：身份、组与 capability 近似。
- [fs](src/sys/fs/README.md)：路径、fd、文件 I/O、事件与文件搬运。
- [ipc](src/sys/ipc/README.md)：signal、futex、eventfd、signalfd 和 SysV SHM。
- [mem](src/sys/mem/README.md)：地址空间、驻留与内存策略。
- [misc](src/sys/misc/README.md)：系统信息、挂载、同步、日志与重启。
- [net](src/sys/net/README.md)：IPv4 TCP/UDP 与 AF_UNIX socket ABI。
- [poll](src/sys/poll/README.md)：poll/select/epoll。
- [task](src/sys/task/README.md)：进程、线程、调度、pidfd 与 wait。
- [time](src/sys/time/README.md)：时钟、睡眠、POSIX timer、timerfd 和 RTC。

## 实现准则

- 未知 flag 必须报错；不能静默忽略会改变正确性的选项。
- 查询/提示允许按 Linux 语义退化，状态修改不能“无操作成功”。
- 阻塞前释放 scheduler 之外的对象锁；用户复制不得发生在自旋锁内。
- 消费型读取先预留、复制成功后提交，`EFAULT` 时恢复数据或 pending 状态。
- 双架构共用 ABI handler，架构差异只放到 platform/MM 后端。

## 用户内存和锁

所有用户地址都不可信：零、非 canonical、跨页、只映射前缀、读写权限变化及并发 unmap 都要变成 EFAULT，不能裸 slice。结构 copy 前先验证固定大小；字符串必须有最大长度和 NUL 规则；iov 数量与总和都要 checked。

禁止在 spin mutex、VFS inode/device 锁、socket 状态锁中触发 user-copy，因为缺页处理可能拿 MM/VFS 锁或失败。典型安全顺序：copy-in 到受限内核值 → 获取对象锁提交 → 解锁 → copy-out。消费型 read 需要 prepare/reserve → 锁外 copy-out → finish/rollback；字符、pipe、socket、eventfd 各自都要保证首字节 EFAULT 不吞数据。

状态修改不可因 copy-out 失败盲目回滚：Linux 有些 syscall 已完成副作用再写结果，具体语义逐项确认。多输出字段应决定逐字段 partial 还是全-or-nothing，并加入跨页 fault 测试。

## 阻塞、signal 与重启

阻塞前必须把 waiter 注册与条件复查做成无丢唤醒协议，然后释放业务锁并调用 scheduler。唤醒后循环复查；timeout 使用统一绝对 deadline，spurious wake 合法。

当前 restart 白名单仅包括 read/readv/write/writev、waitpid/waitid、accept4/connect、send/recv 系列、SysV msg send/recv 和 semop/semtimedop。加入白名单前必须证明：EINTR 返回时没有已向用户报告/不可重复的 partial side effect，重放原参数安全。nanosleep/poll 等有剩余时间或临时 mask 的调用不能机械加入。

trap signal 路径通过 `deliver_pending_signal(frame, restart)` 决定是否改写用户上下文。handler 自己不要偷偷循环吞 EINTR，否则 signal handler 无法运行。线程/进程异常退出统一走导出的 terminate/cleanup 路径，不能从 trap 直接只删 scheduler task。

## 成功后的统计和生命周期

`dispatch_syscall_from_trap` 只对非负结果计 `/proc/<pid>/io`：read family 计读，write family 计写，sendfile/splice/copy_file_range 同时计两侧。partial 正返回按实际字节计；错误不计。统计发生在 handler 返回后，设计上不再持 VFS/设备锁，避免 task-accounting 锁反转。

fork/clone/exec/exit 涉及 syscall 层拥有的 signal、robust futex、fd、timer、IPC 等状态。task 被 wait reap 后还要调用 `drop_reaped_task_runtime_resources(task_id, aspace)`。新增 per-task syscall 状态必须接入 fork copy/share、exec reset、thread exit、group exit、reap 五条生命周期，不能只在成功创建处插入全局表。

## 新 syscall 修改模板

1. 在 API `number.rs` 加官方编号；若已有编号则不要重复。
2. 在合适的 `src/sys/<domain>/` 写 handler，所有整数使用 checked 算术，先拒绝未知 flags。
3. 用统一 user-copy/fallible buffer；明确零长、partial、EFAULT 和 EINTR。
4. 只把验证后的领域值传给组件，集中映射领域错误。
5. 在 `ARG_SYSCALL_TABLE` 登记；返回签名不同才使用 SPECIAL adapter。
6. 若可重启，单独审计后加入 `is_restartable_syscall_nr`。
7. 如返回字节数，确认是否应进入 IO accounting。
8. 接入 fork/exec/exit/reap 生命周期与 self_test。
9. 写用户态裸 syscall 测试，同时在 RV/LA 执行。

一个常见遗漏是只实现 handler、忘记分发表，结果永远 ENOSYS；另一个是登记号大于当前 table max，编译期数组索引会失败或运行时仍越界。提高最大号时应显式调整 table size 到“最大已支持号 + 1”，不要偶然绑定某个 syscall 名。

## 诊断顺序

- 用户见 ENOSYS：先核对架构 ABI 号，再查两张表，最后查 cfg(target_arch)。
- 用户见 EFAULT：检查 user-copy 方向、长度/结构布局、跨页和 copy 前是否已消费数据。
- 卡死：记录阻塞前锁、waitqueue 注册、条件二次检查、signal/timeout 唤醒。
- fork/exit 后 heap 增长：查五条生命周期和全局 registry 是否真正删除 owner。
- 只在 SMP 错：查持锁 user-copy、远端 TLB shootdown、fd/地址空间 owner 和原子发布顺序。

## 总回归矩阵

- 未知号、空槽、RISC-V 专用号在 LA 的 ENOSYS；
- 每个 pointer 参数：零、边界、跨页、只读/只写、并发 unmap；
- 每个长度：0、1、上限、上限+1、加法/乘法溢出、OOM；
- partial read/write、首字节 EFAULT、中途 EFAULT、EINTR/SA_RESTART；
- blocking/nonblocking、timeout=0/有限/无限、spurious wake；
- fork/clone/exec/thread exit/group exit/wait reap 后 fd/MM/IPC/heap 基线；
- RV 与 LA 用户态直调、LTP 对应 case，以及 debug/final profile 差异。

回归程序位于 `user/packages/operator-tools/src/syscall-transfer-smoke.c`，通过
`wos-syscall-test` 在目标机直接发出 asm-generic syscall。
