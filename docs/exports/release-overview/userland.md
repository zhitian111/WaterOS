# userland — 版本概述

当前版本提供 WaterOS 自有、双架构、离线构建的最小用户空间：静态 musl BusyBox、
标准 rootfs 布局、minimal/operator profile、独立 EXT4 镜像和比赛镜像安全叠加。

## 可以完成的工作

- 为 RISC-V64 与 LoongArch64 构建静态 `/bin/busybox` 和 `/bin/sh`。
- 在 QEMU operator 模式运行 shell、文件/文本/进程/基础网络 applet 和 BusyBox vi。
- 用 package/profile 增加后续用户软件，路径冲突默认失败。
- 无 root、无 loop mount 生成可直接作为 QEMU 首块盘的 EXT4。
- 保持基础比赛镜像不变，向副本增加 WaterOS 工具，同时保护 `/glibc`、`/musl`。

## 不包含

- Nano-X、GUI、GPU 或输入设备支持。
- 动态 libc/runtime 构建。
- 传统 PID 1、service manager 或运行时包管理器。
- 自动发现、选择或解释比赛测例。
- 仓库内二进制交叉工具链。

## 最短流程

```bash
make -C user doctor ARCH=rv
make -C user image ARCH=rv PROFILE=minimal
cd os
make shell ARCH=rv PROFILE=pre \
  SDCARD=../user/build/images/wateros-rv-minimal.ext4
```

本工程不会被根目录 `make all` 自动调用，因此比赛内核编译和外部评测镜像仍保持
原有契约。详情见 [`user/README.md`](../../../user/README.md)。

## 修订

| 日期 | 说明 |
| --- | --- |
| 2026-08-09 | 用静态 BusyBox/package/EXT4 系统替换旧用户空间工程 |
