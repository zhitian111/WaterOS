# K-01 `O_RDONLY|O_CREAT` 语义修复结果（2026-08-01）

## 问题与根因

初赛镜像 LTP `ftruncate03` 在准备阶段执行
`open("testfile1", O_RDONLY|O_CREAT, 0644)`，WaterOS 返回 `EINVAL`。Linux 允许该
组合创建空文件并返回只读 fd；后续写入或 `ftruncate` 才应因访问模式失败。

syscall 层已正确把 `O_RDONLY` 转换为 VFS `READ`，错误位于 fs-bridge：不存在的文件
带 `CREATE` 时仍被要求同时具有 `WRITE`；buffered 路径还把 `READ|CREATE` 误判为
无读权限。

## 实现

- paged 与 buffered 普通文件打开路径均允许 `READ|CREATE` 创建空文件。
- 创建动作仍检查挂载可写性，但返回句柄保持 `writable=false` 和 `O_RDONLY` accmode。
- 不存在且未指定 `O_CREAT` 的路径统一返回 `NotFound`，不再因 `O_WRONLY` 隐式创建。
- 未修改 syscall/VFS 公共 API，也未修改 task 模块。

涉及文件：

- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/file_handle.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/paged_handle.rs`

## 验证

- `make check`、`make la_check`、`make kernel-rv-ltp-glibc`：通过。
- 50 秒上限的 RISC-V QEMU 定向回归：7/7 runner case 通过，共 44 个 `TPASS`。
- LTP `open11` 的 28 个 open 组合全部通过，包括普通文件、硬链接和符号链接的
  `O_RDONLY|O_CREAT`，以及目录目标的 `EISDIR`。
- LTP `ftruncate03` 的 socket、只读 fd、负长度和坏 fd 四项 errno 全部通过。
- `fsync03`、`fdatasync02`、`ftruncate01`、`truncate02` 和 5 秒并发 `rename14` 无回归。
- overlay 的 `e2fsck -fn` 五阶段通过；backing 镜像 SHA-256 前后均为
  `74cb5fd3e98a0f14ba9378c48bda3549230d138ab88a5f4060e67ea7cc5a1a24`。

日志：`/tmp/wateros-k01-open-ro-create-20260801.log`、
`/tmp/wateros-k01-open-ro-create-e2fsck-20260801.log`。

## 剩余问题

需要 mount-device 的 LTP 用例仍受缺少 loop/test block device 阻塞。按照白天测试
约束，本次未运行完整 LTP、BuildStorm、iozone 或决赛套件。
