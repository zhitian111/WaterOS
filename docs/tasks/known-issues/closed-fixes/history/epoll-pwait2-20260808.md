# `epoll_pwait2` syscall 441 实现（2026-08-08）

## 问题

RISC-V/LoongArch 的 `epoll_pwait2`（`__NR_epoll_pwait2=441`）此前未注册，LTP
`epoll_pwait02..05` 的 epoll_pwait2 变体全部 TCONF。

## 修改

- `os/components/wateros-syscall/syscall-api/api-v0/src/number.rs`：新增
  `EPOLL_PWAIT2=441`。
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/syscall_nr_dispatch.rs`：
  分发到 `sys_epoll_pwait2`。
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/poll/epoll.rs`：
  - `epoll_pwait2` 使用 `PollDeadline::from_timespec_ptr` 处理 timespec 与
    `EINVAL` 校验。
  - `epoll_pwait`/`epoll_pwait2` 接入现有 `install_poll_sigmask` 临时信号掩码
    guard，并校验 `sigsetsize`，不再只做 128 字节指针探测。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

RISC-V LTP 定向日志 `/tmp/epoll-pwait2-sigmask-fixed.log`：

- `epoll_pwait02`：epoll_pwait / epoll_pwait2 均 TPASS。
- `epoll_pwait03`：两个变体的 timeout 定时均 TPASS。
- `epoll_pwait04`：两个变体的坏 sigmask 均返回 EFAULT。
- `epoll_pwait05`：`tv_sec < 0`、`tv_nsec < 0`、`tv_nsec >= NSEC_PER_SEC`
  均返回 EINVAL。

当前 RISC-V 镜像未包含 `epoll_pwait01` 二进制（`debugfs` 中 inode 为 0），因此
该用例未参与本次定向回归。
