# wateros-driver — 版本概述

## 当前阶段目标

为 WaterOS 内核在 **QEMU virt（RISC-V + OpenSBI / LoongArch64）** 上提供可发现的 **virtio 块盘与网卡**、基础 **串口字符设备**，以及可选的 **写穿块缓存** 与 **smoltcp IPv4 协议栈**，支撑根文件系统挂载、BusyBox/LTP 网络与存储测例。

## 已具备的用户/开发者可见能力

- 启动后自动注册 virtio-blk，devfs 出现 `/dev/vblk*`；ext4 根文件系统可挂载。
- 可选块缓存减少重复 VirtIO 读（评测构建默认开启）。
- virtio-net 注册后，内核可通过 `driver::network::stack` 提供 TCP/UDP socket（配合 syscall 与 VFS）。
- NS16550 UART：控制台之外的字符设备路径；LoongArch/RISC-V 各有平台默认基址。
- RTC/null 字符设备桩满足 `hwclock` 等基础路径探测。
- DTB/PCI 扫描日志与 `driver::test()` 便于 bring-up 诊断。

## 适用范围

- **适用**：`qemu-riscv64-opensbi`、`qemu-loongarch64-virt` 且 QEMU 命令行挂载了 virtio-blk / virtio-net 的构建。
- **部分适用**：仅需编译通过、无真实硬件的 `impl-dummy` 或子系统 `impl-dummy` 路径。
- **不适用**：物理板卡、USB、GPU、完整 Linux 驱动模型、热插拔与电源管理。

## 与系统其它部分的关系

- **`wateros-mm`**：VirtIO HAL 经帧分配器提供 DMA 连续页；`physical_ram_end_exclusive` 喂给恒等映射与帧池上界。
- **`wateros-fs`**：devfs 刷新依赖已注册块设备；驱动不直接实现块存储语义。
- **`wateros-vfs` / syscall**：socket fd 经 `socket_handles` 与 `network::stack` 桥接。
- **`wateros-runtime`**：`serial-uart-virt` 再导出 `driver::uart` 符号。
- **`wateros-platform`**：早期控制台与 driver 层 UART 分工（平台 console vs 字符设备注册表）。

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出 |
