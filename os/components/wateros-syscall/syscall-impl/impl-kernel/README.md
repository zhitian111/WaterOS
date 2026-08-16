# impl-kernel

[返回 syscall 总览](../../README.md)

这是 WaterOS 的 Linux syscall 内核实现。`src/syscall_nr_dispatch.rs` 是唯一按号
分发入口，通过稠密函数指针表把裸调用号直接路由到 handler；`src/sys/` 按领域拆分
handler，用户内存、fd、socket 和 poll 的公共机制放在 `src/` 根，避免各 handler
重复实现。

## 公共基础设施

| 文件 | 作用 |
| --- | --- |
| `user_copy.rs` | 页表感知的用户读写和字符串复制；坏地址返回 `EFAULT`。 |
| `fallible_buf.rs` | syscall 临时缓冲的可失败分配和防御性大小上限。 |
| `vfs_util.rs` / `mm_util.rs` | VFS、MM 领域错误到 errno 的统一映射。 |
| `poll_engine.rs` / `epoll_fd.rs` | poll/select/epoll 扫描、等待和超时。 |
| `socket_fd.rs` / `socket_block.rs` / `unix_sock.rs` | socket fd、阻塞和 AF_UNIX 状态。 |
| `linux_stat.rs` / `stat_times.rs` | Linux stat ABI 与运行时间换算。 |

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

回归程序位于 `user/packages/operator-tools/src/syscall-transfer-smoke.c`，通过
`wos-syscall-test` 在目标机直接发出 asm-generic syscall。
