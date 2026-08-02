# K-08 epoll/BuildStorm runtime 修复结果（2026-08-03）

## 目标与结论

修复 Rust Tokio runtime 在 BuildStorm `tg-xtask` 中创建、复制和关闭 epoll fd 时的
ABI 与生命周期错误。修复后 `cargo metadata`、Tokio runtime 退出和 `tg-xtask` 预构建
均可完成，原先的 `EINVAL` 与退出卡死不再复现。

完整 BuildStorm 尚未通过：计时构建在约 1499 秒后因 guest `rustc` 收到 SIGSEGV
失败。这是后续 MM/并发写入任务，不能计为本项通过。

## 根因与修改

- `fcntl(F_DUPFD/F_DUPFD_CLOEXEC)` 只复制 VFS fd，没有复制 syscall 层 epoll 注册，
  导致复制后的 epoll fd 执行 `epoll_ctl` 返回 `EINVAL`。
- WaterOS 以 Rust `repr(C)` 的 16 字节布局读写 `epoll_event`；Linux RISC-V/LoongArch
  用户 ABI 是 packed 12 字节，造成 data token 和数组步长错位。
- 任意一个重复 epoll handle 关闭时都会清空共享 interests，导致仍存活 handle 的 waiter
  无法被唤醒。现在显式记录 handle 引用，仅最后一个 handle 关闭实例。

涉及文件：

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/epoll_fd.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/fcntl.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/poll/epoll.rs`

## 验证

- `make rv_check`：通过。
- `make la_check`：通过。
- `make kernel-rv-final-log`：通过。
- 新主办方 RISC-V 镜像、8 CPU、8 GiB、干净 qcow2 overlay：CAgent 10/10，约
  3.38 秒。
- 517 KiB `cargo metadata` 输出分别经 pipe、shell command substitution 和普通文件
  重定向：均成功，pipe/command substitution 返回 0。
- 正式 BuildStorm：toolchain、minibuild、`tg-xtask` 预构建通过；预构建约 81 秒，
  已进入 ArceOS 内部 release 构建。
- 测试后 overlay `e2fsck -fn`：无结构性错误，仅报告 extent tree 可优化。

原始日志：`/tmp/wateros-final-formal-20260803.log`（不提交）。后续首个失败是
`compiler_builtins` 的 `rustc` SIGSEGV；`/work/buildstorm.build.out` 还出现前 12 KiB
变为 sparse hole 的现象，需在 K-01/K-07 中继续定位。
