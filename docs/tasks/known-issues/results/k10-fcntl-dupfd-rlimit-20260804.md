# K-10 `F_DUPFD` 资源边界修复报告（2026-08-04）

## 现象

LoongArch-musl LTP 全量运行到 `crash02` 时，确定性随机序列
`crash02 -s 45` 调用：

```text
fcntl(fd=0, F_DUPFD, minfd=0x9d90d244)
```

内核随后尝试分配约 96 MiB，触发 128 MiB 全局堆的 alloc-error panic。`crash02` 是用
随机参数检查 syscall 健壮性的用例，不能用过滤掩盖该内核资源漏洞。

## 根因

`PerTaskFdRegistry::install_dup_fd_for_task()` 只检查当前已打开 fd 数量是否达到
`RLIMIT_NOFILE`，没有检查请求的 `minfd`。随后代码用 `while table.len() < minfd`
扩展稀疏 `Vec<Option<...>>`，不可信用户参数可直接耗尽内核堆。

## 修改

- syscall 层的 `F_DUPFD` 和 `F_DUPFD_CLOEXEC` 在 `minfd >= RLIMIT_NOFILE` 时按
  Linux `fcntl(2)` 语义返回 `EINVAL`。
- fd registry 在扩容前重复检查 `minfd`，越界返回 `TooManyOpenFiles`。这是独立于
  syscall 调用者的资源所有权防线，避免其他 VFS 调用路径制造超大表。
- VFS 自测增加边界断言，确认拒绝 `minfd == RLIMIT_NOFILE`。

该修改保持 fd table、OFD、fork 共享和 task 生命周期接口不变。

## 验证

- `make rv_check`、`make la_check` 和两架构 LTP-musl 内核构建通过。
- LoongArch64/QEMU 8 核：`crash02 -s 45` 完成 100 次随机 syscall，1 TPASS，退出 0。
- RISC-V64/OpenSBI 8 核：同 seed 完成，1 TPASS，退出 0。
- 两架构均无 heap OOM、panic，runner 正常结束。

日志 SHA-256：LoongArch `be37288d...50171`，RISC-V
`49e5ea56...1dc00`。首次全量失败日志为
`/tmp/wateros-k02b-la-musl-ltp-full-after-fixes.log`（`585d1d73...ba7e5`）。
