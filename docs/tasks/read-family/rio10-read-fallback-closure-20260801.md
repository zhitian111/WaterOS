# RIO-10 阶段记录：关闭破坏性 read fallback

## 结果

`sys_read(2)` 与 `sys_readv(2)` 现在都强制使用 `VfsPreparedRead`/`VfsReadLease`，不再在
`prepare_read` 返回 `Unsupported` 时退回“先读入内核缓冲、再复制用户空间”的旧路径。
因此后续新增句柄不能静默绕过 RIO-04 至 RIO-09 的 reserve/copy/commit 契约。

静态审计确认 regular/proc 文件、pipe、Unix socket、inet TCP/UDP、eventfd、字符设备、
zero/null/urandom 和 console input 均实现 `prepare_read`。未实现该接口的对象是不可读
输出端、epoll 和 TCP listener；它们继续通过访问检查或 `Unsupported -> EINVAL` 失败，
不需要破坏性 fallback。

## 修改文件

```text
os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/io.rs
```

删除了 `read_fd_legacy`、通用破坏性 `read_fd` 以及 TCP/UDP legacy blocking read helper，
同时移除了逐调用 read trace。网络写路径使用的 poll helper 保留。

## 验证

- `make rv_check`：通过。
- `make la_check`：通过。
- `make kernel-rv-ltp-glibc`：通过。
- RISC-V 定向 QEMU：`read01`、`read02`、`recv01`、`eventfd01` 全部退出 0。
- 日志 `/tmp/wateros-rio10-no-legacy-read.log` 同时发现两个独立既存问题：`pipe03` 的
  pipe 读端 write errno 错误，以及 `socketpair01` 的 Unix datagram/errno 不完整；二者
  均发生在 read lease 前，另行修复。
- 临时镜像入口和注入 runner 已恢复/删除，并通过 `cmp`。

按白天约束未运行完整回归矩阵。
