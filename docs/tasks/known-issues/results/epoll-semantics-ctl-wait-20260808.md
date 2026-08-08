# epoll `ctl`/`wait` 语义修复（2026-08-08）

## 问题

LTP epoll 组在 ABI 修正后仍有这些失败：

- `epoll_wait03`：负数 `maxevents` 被当作大正数，`epoll_wait` 返回成功而不是
  `EINVAL`。
- `epoll_ctl02`：合法但不可 poll 的 fd（目录）应返回 `EPERM`，内核返回 `EBADF`。
- `epoll_ctl03`：`events=0` 被误判为非法。
- `epoll_wait05`：TCP `shutdown(SHUT_RD)` 后未上报 `EPOLLRDHUP`。
- `epoll_wait06/07`：`EPOLLET` 和 `EPOLLONESHOT` 没有状态，导致事件被重复上报。

## 修改

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/poll/epoll.rs`：

- `epoll_wait` 的 `maxevents` 先按 `isize` 解析，`<= 0` 返回 `EINVAL`。
- `epoll_ctl` 对“有效 fd 但不可 poll”返回 `EPERM`，无效 fd 仍返回 `EBADF`。
- `events=0` 属于合法事件组合，只拒绝超出 `EPOLL_VALID_EVENTS` 的位。
- `EpollInterest` 增加 `edge_ready` / `oneshot_armed` 状态；`EPOLLONESHOT`
  上报一次后停用，`EPOLL_CTL_MOD` 重新武装。
- `EPOLLET` 按就绪边沿上报；pipe 的 `EPOLLOUT|EPOLLET` 与 Linux 实测一致，只有
  pipe 缓冲清空时才重新上报，而不是只要有空间就上报。

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/epoll_fd.rs`：

- `EPOLLRDHUP` 映射为 `POLLRDHUP`，`EpollInterest` 增加边沿/一次性状态字段。

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/poll_engine.rs`：

- 新增 `POLLRDHUP`；TCP socket 在远端读半关闭或本地 `shutdown(SHUT_RD)` 后上报。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

LTP 定向日志：

```text
epoll_ctl02 : 9 TPASS, FAIL LTP CASE epoll_ctl02 : 0
epoll_ctl03 : 2048 TPASS, FAIL LTP CASE epoll_ctl03 : 0
epoll_wait03 : 5 TPASS, FAIL LTP CASE epoll_wait03 : 0
epoll_wait05 : Received EPOLLRDHUP, FAIL LTP CASE epoll_wait05 : 0
epoll_wait06 : 12 TPASS, FAIL LTP CASE epoll_wait06 : 0
epoll_wait07 : 5 TPASS, FAIL LTP CASE epoll_wait07 : 0
```

日志：`/tmp/epoll-ctl-fixed.log`、`/tmp/epoll-edge-fixed.log`。

## 后续

`epoll_pwait2`（syscall 441）仍为 `TCONF`，需要在实现 `epoll_pwait2` 时补充；
当前 epoll 系列其余 LTP 用例均已通过。
