# T11：真机适配（VisionFive2 / LS2K1000）

## 任务内容

把 T01–T04 的 irqchip/irq-domain/DTB 抽象落到真机板级实现。此任务阻塞于板子到手，
先只做设计对齐；上板后再补实现与验证。

## 实施方案

1. VisionFive2：RISC-V SiFive PLIC（结合历史分支 `feat/visionfive2-port` 的 PLIC topology/context 工作）。
2. Loongson 2K1000：LIOINTC + EIOINTC（结合历史分支 `feat/loongson2k1000-port` 的 IRQ domain / MMC DMA 工作）。
3. 复用 DTB irq-domain 解析，验证不同 `interrupt-parent`/`interrupt-map` 拓扑都能得到稳定 `virq`。
4. 真机验证：bring-up、块/网卡中断完成、MMC/DMA 完成路径。

## 涉及文件

- 对应 `platform-impl/impl-*` 的 irqchip 模块
- `driver-impl/impl-*` 的设备 IRQ 注册
- `components/wateros-irq`（如需扩展 hierarchy/domain 能力）

## CodeGraph 查询命令

```bash
codegraph explore "IrqChip IrqDomain interrupt-map"
codegraph explore "plic topology context"
codegraph explore "eiointc liointc irq domain"
```

## 验收方式

- 板级 bring-up 日志、设备中断完成、无卡死。
- 上板验证前，本任务仅做设计/文档与 QEMU 等价性 check。

## 验收命令

上板命令以届时硬件环境为准；QEMU 侧先跑 `make rv_check && make la_check`。

## 任务简报

完成后写 `history/11-real-hardware-port.md`。
