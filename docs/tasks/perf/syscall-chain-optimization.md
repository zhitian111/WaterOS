# 系统调用链路低风险性能优化

## 状态

审计基线：`a6cf2515`（分支 `perf/syscall-chain-opt`）。

本任务只进入不改变 Linux ABI、阻塞/唤醒顺序、文件偏移线性化点和锁顺序的优化。每个子项
单独实现、单独验证、单独提交，便于回滚。完整 QEMU 验收由主分支合入前另行执行。

## 第二轮审计范围

沿以下链路逐层检查了当前实现：

```text
trap_handler
  -> dispatch_syscall_from_trap
  -> dispatch_syscall_by_nr
  -> sys::*
  -> user_copy / task / vfs::fd / poll_engine / socket / ipc
```

重点覆盖：

- trap 与 syscall 分发：`wateros_kernel_trap_handler`、双函数表分发、统计钩子；
- 用户地址空间访问：路径字符串、结构体、iovec、poll/select/epoll 输出；
- fd/VFS：registry 锁、`SharedIoHandle` 锁、socket 类型识别、阻塞 I/O detached handle；
- 文件 I/O 与搬运：read/write/readv/writev/preadv/sendfile/splice/vmsplice；
- poll/select/epoll、futex、signal、clone/fork 的循环、锁和复制边界；
- syscall 侧的 `Vec`/`BTreeMap`/`BTreeSet` 使用及可见的重复查找。

## 已确认的低风险任务

### T1：缓存 `poll/select` 用户输入

状态：已完成，提交为 `[perf] cache poll and select userspace inputs`。

实现结果：`poll/ppoll` 在入口导入一次 `PollFd` 数组，等待扫描复用内核副本；
`select/pselect6` 在入口导入三份 fdset，等待阶段不再重复访问用户地址空间，结果仍按原路径
写回用户空间。输入数组使用 checked offset 和 `try_reserve_exact`，空指针、长度及错误返回
保持原有边界。`rv_check` 已通过（`CARGO_NET_OFFLINE=true make rv_check`）。

现状：

- `poll` 每轮扫描逐项 `copy_from_user_struct`；等待轮次会重复导入同一份 `(fd, events)`；
- `select/pselect6` 的 `scan_fd_sets_inner` 每轮复制三份 fdset；
- `fd_monitored_in_sets` 在等待阶段对每个 fd 再复制三份完整 fdset。

位置：`poll_engine.rs` 的 `scan_pollfds`、`poll_block_until_ready`、
`scan_fd_sets_inner`、`fd_monitored_in_sets`。

方案：syscall 入口一次性导入用户输入，内核中保存不可变监视集合和可变结果集合；等待期间
只扫描内核数据，返回前一次性写回 `revents`/fdset。保留所有空指针、长度、EFAULT、EBADF
和超时语义。

风险边界：不持有 fd registry 锁跨越等待；每次 readiness 检查仍调用现有 handle/socket
接口；只缓存用户输入，不缓存可能变化的设备状态。

### T2：合并 poll 路径的 socket 分类查找

状态：已完成，提交为 `[perf] reuse classified sockets during poll scans`。

实现结果：每次就绪扫描只重新分类一次 fd；已识别的 `SocketRef` 直接传入 readiness 计算，
不再重复经过 fd registry。普通 fd、无效 fd 和跨等待轮次的重新分类保持原有路径。`rv_check`
已通过（`CARGO_NET_OFFLINE=true make rv_check`）。

现状：同一轮 socket readiness 检查可能经过 `scan_pollfds -> socket_fd::lookup ->
poll_revents_fd -> socket_fd::lookup -> poll_socket_revents -> socket_fd::lookup`。

方案：让 poll readiness helper 接受已经取得的 `Option<SocketRef>`，普通 fd 走一次 VFS
handle 查询；等待路径仍使用 detached handle，不能把共享 fd 槽锁带入 sleep。

风险边界：socket 对象仍由 `Arc` 持有；不改变 `SocketRef` 内部协议栈 mutex；epoll 实例
仍先复制 interest 快照，不在实例锁内执行 fd readiness。

### T3：路径 C 字符串按块复制

状态：已完成，提交为 `[perf] copy user paths in bounded chunks`。

实现结果：`copy_user_path_cstr` 以 64 字节块读取，正常路径每块只做一次用户地址空间访问；
块复制遇到跨页错误时回退到该块的逐字节读取，保留有效前缀中的 NUL、`EFAULT` 和边界语义。
`rv_check` 已通过（`CARGO_NET_OFFLINE=true make rv_check`）。

现状：`copy_user_path_cstr` 每次只复制 1 字节，最长路径最多进行 4096 次 MM copy。

方案：按固定小块复制到内核临时缓冲，在内核中寻找 NUL；最后一个块允许提前结束。保持
`EFAULT`、`EINVAL`、`ENAMETOOLONG`、UTF-8 检查和最大长度含终止符的定义。

### T4：批量导入 iovec 元数据

现状：`readv/writev/preadv/pwritev/vmsplice/sendmsg/recvmsg` 等路径逐个复制用户 iovec
结构，每项都重新获取当前地址空间 handle。

方案：先做经过溢出检查的整体范围复制，再在内核数组中解析；必要时按目标架构布局使用
`copy_from_user_in_aspace`。不改变 iovec 的逐项地址/长度校验和部分成功语义。

注意：`writev/sendmsg` 将 iovec 内容拼成连续 buffer 是更大的额外复制，但消除它需要
vectored VFS/socket API，暂不在本轮改公开契约；本任务只处理元数据导入和重复地址空间查找。

### T5：合并 `/proc/<pid>/io` 的双重统计锁

现状：`sendfile/splice/copy_file_range` 成功后分别调用一次 read 统计和一次 write 统计，
连续获取同一个 cwd registry 锁。

方案：增加一个内部的双方向统计入口，在一次锁临界区内更新四个计数器。普通单方向统计
保持原有行为。

### T6：有限范围复用 fd context

现状：`read/write` 先做 access/path/socket 检查，随后再次查同一个 fd；`is_nonblocking`、
TTY/PTY 检查也会重复进入 registry。

方案：只在不跨阻塞点的 syscall 前半段引入短生命周期 context，缓存 `SharedIoHandle`/socket
引用和只读元数据；实际可能阻塞的 I/O 继续使用现有 detached/prepared API。若无法证明锁和
文件偏移语义不变，则只应用 T2 的 socket readiness 去重，不扩大改动。

## 明确暂不实施的候选项

### vectored I/O API

`writev/sendmsg/pwritev` 当前把用户 iovec 内容拼成连续 buffer。消除这次复制需要扩展
VFS/socket 公共 trait，并重新定义短写、偏移和错误后的 iovec 进度。本轮不改公开契约，
只记录为后续独立任务。

### detached handle 复制

`with_current_io_detached` 的复制用于防止 pipe/socket 阻塞时持有共享 fd 槽锁，避免单核
死锁。不能因为“多了一次 duplicate”就删除。

### epoll 实例锁和线性容器

epoll 当前先复制 interest 快照，再逐项查询 readiness，避免在 fd/网络操作期间持有 epoll
锁。直接改为持锁扫描有死锁和 ctl 并发风险。`BTreeMap` 替换为 hash 容器也会改变 no_std
依赖、分配和确定性，当前没有 profile 证据，不纳入本轮。

### futex/clone/signal 状态表

审计未发现可以不改变线性化点就删除的锁或复制；这些路径的重复读取多数是为处理用户页
缺页、SMP 竞态或 wait/wake 竞态而存在，暂不动。

## 实施顺序与验证

1. T1：`poll_engine` 用户输入缓存；运行 syscall-impl-kernel 相关测试/check。
2. T2：poll socket 分类查找去重；运行同一 crate check。
3. T3：路径分块复制；增加边界单测并运行 syscall-impl-kernel check/test。
4. T4：iovec 元数据批量导入；运行 readv/writev/sendmsg/transfer 相关测试/check。
5. T5：I/O 统计合锁；运行 syscall 与 VFS 受影响 crate check/test。
6. T6：只有在前五项验证后仍能证明 context 复用不跨阻塞点时实施；否则保留 T2 结果并将
   T6 标记为 deferred。

每一项完成后：

- `cargo fmt --check` 或受影响 crate 的等价格式检查；
- 受影响 crate 的 `cargo check`/`cargo test`；
- `git diff --check`；
- 单独提交，提交信息使用 `[perf] ...` 或 `[fix] ...`，不混入生成物。

## 审计结论

当前最确定的性能损失来自用户输入在等待循环中的重复复制、路径逐字节 MM 访问、以及
syscall 热路径上的重复 fd registry lookup。syscall 双表分发、统计 atomic、detached
handle 和 epoll 快照均不是本轮应优先改动的对象。
