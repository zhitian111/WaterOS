# wateros-fs — 阶段能力概述

## 定位

`wateros-fs` 是 WaterOS 的**文件系统实现层**：负责块设备上的 ext4 读写、设备节点枚举、proc 伪文件与根卷生命周期。它面向内核 bring-up 与 `wateros-vfs` bridge，不直接承载 syscall 的 fd 表语义。

## 当前阶段已具备

- 启动时自动探测 QEMU 根块设备并绑定 ext4（`ext4_rs`）实现。
- 单一 RW 根卷挂载路径，支持 bring-up 写入与测例脚本依赖的目录/文件操作。
- devfs 提供块设备路径与默认根设备选择。
- procfs 提供进程/status/maps/mounts 等 LTP 常用只读节点。
- 可选第二 ext4 卷 RO/RW 挂载（不替换根句柄）。
- 统一的 `FsError` 与 `FsImpl` 注册，便于切换 `impl-ext4-rs` 与旧 `impl-ext4`。

## 适用范围

- QEMU virt 上的 ext4 根镜像（sdcard 镜像流程见 `os/Makefile` `flush_img`）。
- 与 `wateros-vfs` bridge 联调的单核 bring-up。
- 需要 devfs + ext4 + procfs 组合的用户态测例。

## 尚未作为产品目标的部分

- 多文件系统类型并存于根路径（btrfs、xfs 等）。
- 完整 journal 崩溃恢复与 fsck 工具链。
- 生产级 ext4 特性（extent 优化、大文件压力、并发写一致性）。
- 独立 mount namespace 与每个任务不同根卷。

## 与其它组件的关系

- **被** `wateros-vfs`（bridge）、`os/src` bring-up 调用。
- **依赖** `wateros-driver` 块设备 API、`wateros-runtime-logging`。
- **不向** syscall 层直接暴露；经 VFS 聚合访问。
