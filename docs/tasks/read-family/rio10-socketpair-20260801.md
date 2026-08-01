# RIO-10 问题修复：socketpair errno 与 Unix datagram

## 问题

RIO-10 runner 中 `socketpair01` 仅 4/10 通过。当前实现先拒绝非 AF_UNIX domain，导致
AF_INET 的 invalid type/protocol 全部返回 `EAFNOSUPPORT`；AF_UNIX `SOCK_DGRAM` 也被
直接返回 `EPROTONOSUPPORT`。

## 修复

- `socketpair` 先剥离 `SOCK_NONBLOCK/SOCK_CLOEXEC` 并验证基础 type，再按 domain、
  type 与 protocol 组合返回 `EINVAL`、`EAFNOSUPPORT`、`EPROTONOSUPPORT` 或
  `EOPNOTSUPP`。
- AF_UNIX `SOCK_DGRAM` 创建两个独立 `DgramInbox`，端点保存对端 inbox 引用；write 和
  无显式地址的 sendto 直接向对端投递 record。
- datagram pair 复用既有 inbox 容量、waitqueue 和 read reservation；关闭一端会关闭其
  inbox，使对端后续发送得到连接拒绝，而 dup/fork 继续共享同一 socket inner。
- stream/seqpacket pair、CLOEXEC 和 NONBLOCK 路径保持原实现。

涉及文件：

```text
os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/socketpair.rs
os/components/wateros-syscall/syscall-impl/impl-kernel/src/unix_sock.rs
```

## 验证

- `make rv_check`、`make la_check`、`make kernel-rv-ltp-glibc`：通过。
- LTP `socketpair01`：10 TPASS、0 TFAIL。
- LTP `socketpair02`：4 TPASS、0 TFAIL。
- QEMU runner 汇总：2 passed、0 failed、0 missing；日志
  `/tmp/wateros-socketpair-fix.log`。
- 镜像入口和注入 runner 已恢复/删除并通过 `cmp`。

现有 LTP 只验证 datagram pair 创建；实际 datagram 收发、fault rollback 和并发 close
仍纳入 RIO-10 夜间完整门禁。未修改 task 或 scheduler。
