# WaterOS VisionFive 2 物理端口最终现状报告

日期：2026-08-10  
工作树：`WaterOS_visionfive2_port`  
分支：`feat/visionfive2-port`

## 当前状态

VisionFive 2 的板级适配已经完成“可安全尝试 bring-up”的软件准备，但尚未解除硬件证据闸门。

已完成：

- JH7110 DTB topology：chosen UART、PLIC、MMC、clock/reset/syscon resource 解析；
- UART `reg-shift`/`reg-io-width` 到 Byte16550/DW APB32 的白名单映射；
- PLIC supervisor context 唯一选择、disabled/空 context/非法 source/MMIO fail-closed；
- MMC bus width、调优参数、FIFO 深度和静态资源校验；
- SD block 注册前容量、首扇区读取和分区链路检查；
- 所有真实 clock/reset/pinmux/card/IRQ/MMC register 行为仍由 `HardwareEvidence` 显式门控。

## 已有验证证据

- VisionFive 2 driver crate 单元测试已覆盖 UART、PLIC、MMC、分区注册和拓扑边界；最新拓扑批次为 18 项通过；
- RISC-V64 目标 `cargo check` 通过；
- 所有测试均不执行真实 MMIO，不会在缺板时伪造硬件成功。

## 仍待实体板验证

JH7110 UART 时钟/pinmux/波特率、PLIC 地址译码与 context/hart 路由、MMC clock/reset/syscon、SD 供电和 card detect、MMC IRQ、FIFO/PIO 时序、真实 SD 读写及 DMA/cache 行为。当前不会自动注册未经实测的 SD block 设备。

## 板子到位后的安排

1. 以最小启动镜像确认 DTB、UART 输出和 hart/PLIC context。
2. 逐步实测 clock/reset/pinmux，再解除 `HardwareEvidence` 对 MMC 的阻断。
3. 用小容量 SD 卡验证 CID、容量、单块/多块读写和 GPT/MBR 分区。
4. 再启用网络与输入设备，记录 IRQ、DMA/cache 和 devfs 节点变化。
5. 将实测日志和解除的 blocker 回填本工作树进度文件，再合并共有分支。

本工作树至此暂停，等待 VisionFive 2 实板。
