# K-08 epoll/FD 锁序修复结果（2026-08-03）

## 结论

修复 BuildStorm 中 epoll 扫描与 fork fd 表复制形成的 ABBA 锁等待。旧路径一侧持有
epoll instance 锁后查询具体 fd，另一侧持有全局 fd registry 锁并复制 epoll handle，
两者可互相等待。

## 修改

- epoll readiness 扫描只在 instance 锁内复制 interest，具体 fd poll 在锁外执行。
- fork 分三阶段复制 fd 表：registry 锁内快照、锁外 duplicate concrete handle、registry
  锁内安装子表。
- 保持 fd slot、flags、shared open-file-description 和 epoll interest 的既有语义。

涉及文件：

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/epoll_fd.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/poll/epoll.rs`
- `os/components/wateros-vfs/src/fd.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fd-session/src/registry.rs`

## 验证

- `make check`：通过。
- 新主办方 RISC-V 镜像、OpenSBI、8 CPU、8 GiB 的 final 运行超过两小时，越过原约
  6 分钟锁等待点；CPU/任务快照未再出现 epoll instance 与 fd registry 互等。
- BuildStorm 已运行到最后一个 `arceos-helloworld` 编译阶段。
- 后续独立失败为 `rustc` 进程残留在线程退出 futex 等待；不属于本项锁序问题。
- 停机后对镜像执行 `e2fsck -fn`：无结构损坏，仅有 extent tree 可优化提示。
