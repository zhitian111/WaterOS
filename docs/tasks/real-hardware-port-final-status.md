# WaterOS 共有物理端口工作树最终现状报告

日期：2026-08-10  
工作树：`WaterOS_real_hardware_ports`  
分支：`feat/real-hardware-common`

## 当前状态

共有软件适配已完成到可交给实体板 bring-up 的阶段。根文件系统、分区、devfs、VirtIO DMA 和远程诊断均已有代码或 QEMU/单元测试证据。

已完成的主要工作：

- rootfs manifest/staging、GPT/MBR 镜像构建和小镜像 QEMU smoke；
- MBR/GPT 分区边界、主备 GPT 回退、分区注册与父盘注销级联；
- devfs 对 block、character、input 设备的动态刷新、稳定 slot 和旧节点清理；
- VirtIO block/network/input/display 的连续 DMA 分配、释放和地址边界收口；
- `OwnedPhysFrameSpan` 到 DMA region 的公共适配；
- 无认证、仅开发用途的 TCP remote monitor，以及 QEMU 端到端协议 smoke。

## 已有验证证据

- RISC-V 和 LoongArch 的 `make check`/交叉编译路径已覆盖共有 DMA 与驱动 glue；
- 小型 GPT 镜像已在 QEMU 验证 block 注册、devfs 刷新、`/dev/vda1` 根挂载和 runner；
- `operator-shell` 专用 kernel + 16 MiB 镜像已验证 remote monitor 的 hello/capabilities/ping/status/version/quit；
- 脚本单测、分区测试、DMA API 测试和 devfs 动态注销测试已提交；
- 第三方依赖许可证清单已记录。

## 仍待实体板验证

真实 SD/eMMC 时序、UART/PHY、电源和热插拔、中断路由、DMA cache coherency、IOMMU、设备停止后的释放时序，以及 remote monitor 在真实网络上的可达性。相关路径均保留 `UNVERIFIED_ON_HARDWARE` 说明，未把 QEMU 结果冒充真机证据。

## 板子到位后的安排

1. 先按 VisionFive 2/Loongson 各自报告烧录最小镜像，确认串口和 DTB。
2. 逐项启用 block、network、input/display，记录 IRQ、DMA、cache 和 reset 结果。
3. 运行 rootfs、分区、网络、输入和 remote monitor 回归。
4. 将两个平台分支合并到共有分支，保留硬件差异和实测日志。

本工作树至此暂停，不再继续扩展共有代码。
