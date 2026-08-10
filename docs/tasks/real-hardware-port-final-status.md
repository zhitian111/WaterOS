# WaterOS Loongson 2K1000 物理端口最终现状报告

日期：2026-08-10  
工作树：`WaterOS_loongson2k1000_port`  
分支：`feat/loongson2k1000-port`

## 当前状态

Loongson 2K1000 的板级适配已完成资源描述、AHCI/GMAC 证据契约和 DMA 映射接口，当前适合等板后进行分阶段 bring-up。

已完成：

- LoongArch 启动参数、DTB 保存和平台资源边界；
- AHCI ABAR 基址/尺寸/端口资源校验及硬件证据闸门；
- GMAC metadata、MMIO、IRQ、descriptor/环形缓冲约束和硬件证据闸门；
- `OwnedDmaBuffer` 显式虚拟地址入口及 identity wrapper 兼容路径；
- LoongArch 目标编译和 DMA/GMAC/AHCI 纯逻辑测试。

## 已有验证证据

- Loongson driver 相关 host 单测已覆盖 AHCI、GMAC、DMA metadata 和错误路径；
- LoongArch 目标 `cargo check` 通过；
- DMA buffer 的地址、长度、对齐和设备地址宽度约束已在无硬件环境中验证；
- 所有真实控制器访问仍受硬件证据 contract 保护。

## 仍待实体板验证

AHCI BAR 实际映射、PCI/中断路由、SATA 设备识别和读写、GMAC PHY/link/autoneg、DMA 地址可见性、cache coherency、descriptor ownership、reset/stop/release 及真实网络吞吐。当前不宣称 AHCI 或 GMAC 已可在实体板工作。

## 板子到位后的安排

1. 先确认串口、DTB、PCI/AHCI BAR 和外部中断。
2. 以无写入的 SATA/存储探测开始，随后做小范围块读写和分区识别。
3. 连接已知可用 PHY，验证 GMAC MAC/PHY、链路和单帧收发。
4. 逐项记录 DMA buffer 的 VA/PA、cache flush/invalidate、descriptor 回收和设备停止行为。
5. 将硬件证据结果回填日志，解除相应 contract 后再合并共有分支。

本工作树至此暂停，等待 Loongson 2K1000 实板。
