# 任务 07：close/dup 复用 FD slot 资源分类

## 任务内容与目标

去掉 close、dup、fcntl duplicate 对 PTY、Unix socket、epoll 和 inode lock 的多路负向探测，
让取出/替换 slot 时一并返回分类和必要的清理 key。所有清理仍在 registry 锁外执行。

## 实施方案

1. close/take/dup3 replace 返回 typed slot snapshot，而非仅 handle。
2. 按分类执行 PTY event、Unix unregister、epoll remove；普通文件直接跳过无关侧表。
3. 在稳定普通文件 slot 中缓存 inode lock key，避免 close 再次 metadata。
4. 保持 Linux close 已移除 fd 后不回滚的错误语义，以及 dup3 原子替换语义。
5. 覆盖 close_range、exec CLOEXEC、fork shared table 和重复 fd 替换测试。

## 涉及文件

- `os/components/wateros-vfs/src/fd.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fd-session/src/{registry,file_lock}.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/{close,dup,fcntl}.rs`
- `unix_sock.rs`、`epoll_fd.rs`、TTY 清理适配

## CodeGraph 查询

```bash
codegraph explore "sys_close close_fd dup3_fd release_locks_for_current_process"
codegraph callers "close_fd"
codegraph impact "take_fd_for_close"
```

## 验收方式

```bash
cd os
make rv_check && make la_check && make kernel-rv-final
# close/dup/close_range、PTY hangup、Unix socket、epoll 生命周期回归
cd .. && git diff --check
```

普通文件 close 不再查询 PTY/Unix/epoll 或 metadata；资源析构测试无泄漏、无重复 unregister。
任务 00 runner 验证 BuildStorm open/close 密集阶段无回退。

## Commit 与简报

提交建议：`[perf] close 与 dup 使用 FD slot 分类清理`。新增 `history/07-brief.md`。
