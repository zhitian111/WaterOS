# wateros-vfs — 阶段能力概述

## 定位

`wateros-vfs` 是 WaterOS 内核中面向 syscall 与 bring-up 的**虚拟文件系统门面**：统一路径解析、打开/读写、挂载命名空间与 per-task 文件描述符，底层块文件系统与设备树由 `wateros-fs` 提供。

## 当前阶段已具备

- 以 ext4 为默认根卷，经 bridge 提供 open/read/write/seek 与目录操作。
- 多卷挂载：块设备 ext4、内存 tmpfs、procfs、devfs 路径、bind mount 与传播类型子集。
- 标准 fd 0/1/2、动态 fd、pipe 与 Unix stream pair。
- 常见路径 syscall 辅助：`chmod`、`xattr`、`symlink`、`rename`、`truncate` 等绝对路径 API。
- 文件页缓存减轻小块读写的块设备压力。
- 组件级 `vfs::test()` 自检链，适合 QEMU bring-up 日志观察。

## 适用范围

- RISC-V / LoongArch QEMU bring-up 与 LTP 子集测例。
- 单核、单地址空间假设下的 fd 与 cwd 管理。
- 需要与 Linux 用户态路径语义大致对齐的场景。

## 尚未作为产品目标的部分

- 完整 VFS 缓存一致性、多核并发挂载。
- 网络文件系统、FUSE、完整 inotify。
- 与 Linux 完全一致的 mount 命名空间复制（clone flags 子集）。
- 不启用 `bridge-fs-api` 时仅有 dummy 后端，不能运行真实用户态文件 I/O。

## 与其它组件的关系

- **依赖** `wateros-fs`（bridge）、`wateros-task`（fd/cwd）、可选 `wateros-base`。
- **被** `wateros-syscall`、bring-up、`os/src/main.rs` 自检调用。
