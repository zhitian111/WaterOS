# 任务 06：read/pread 家族使用一次性 FD I/O lease

## 任务内容与目标

让 read、pread64、readv 和 preadv2 在一次 FD registry 查询中取得 handle、slot flags、
资源分类、nonblock 和 terminal 信息，随后在 registry 锁外完成校验、等待、I/O 与用户拷贝。
普通文件路径不得再做 socket/TTY 负向探测。

## 实施方案

1. 新增 `FdIoLease`/`PreparedFdIo`，在 registry 锁内只 clone slot 快照，绝不阻塞。
2. 用缓存分类完成 O_PATH、读权限、socket wait、TTY job-control 分派。
3. 复用现有 `VfsPreparedRead` 与 partial-EFAULT/offset reservation 语义；不要在 slot 锁内睡眠。
4. socket、pipe、TTY 的 EAGAIN/EINTR/SA_RESTART 行为保持不变。
5. 将 read 家族公共入口收口，避免 read 优化而 readv 仍走旧锁链。

## 涉及文件

- `os/components/wateros-vfs/src/fd.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fd-session/src/registry.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/io.rs`
- socket/TTY 等等待适配模块及定向测试

## CodeGraph 查询

```bash
codegraph explore "sys_read sys_pread64 prepare_current_read validate_read_fd check_tty_foreground"
codegraph callers "prepare_current_read"
codegraph impact "validate_read_fd"
```

## 验收方式

```bash
cd os
make rv_check && make la_check && make kernel-rv-final
# read/pread/readv/preadv、pipe、socket、PTY、O_PATH、EFAULT 定向回归
cd .. && git diff --check
```

任务 01 计数证明普通文件每次 read 只有一次 FD registry snapshot，socket/TTY 语义回归通过。
用任务 00 runner 交错 A/B；若锁进入减少但墙钟无收益，简报必须记录下层瓶颈。

## Commit 与简报

提交建议：`[perf] read 家族统一使用 FD I/O lease`。新增 `history/06-brief.md`。
