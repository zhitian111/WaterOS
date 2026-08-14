# T03：LoongArch EIOINTC + PCI interrupt-map + ESTAT/ECFG

## 任务内容

在 LoongArch QEMU virt 上打通外部中断链路。LoongArch 当前块/网卡走 virtio-PCI，
因此必须同时处理 PCI INTx/MSI-X 与 EIOINTC 的路由关系，是本任务风险最高的一环。

## 实施方案

1. arch 层：
   - `os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64/src/trap.rs`
     增加 ESTAT 外部中断 decode（当前只解 IPI/timer）。
   - `os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64/src/interrupt.rs`
     增加 `ECFG` 外部中断使能。
2. platform-impl 增加 EIOINTC irqchip：
   - 新文件 `os/components/wateros-platform/platform-impl/impl-qemu-loongarch64-virt/src/eiointc.rs`；
   - 从 DTB `pci@*`/`eiointc` 节点解析 `interrupt-map`，映射 PCI INTx 到 EIOINTC 线；
   - 若 QEMU/设备使用 MSI-X，确认 `virtio-drivers` PCI 的 MSI-X 配置路径并接线，否则先支持 INTx。
3. `os/src/trap_handler.rs` 的 LoongArch 外部中断分支复用 T02 的 irq 分发。

## 涉及文件

- `os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64/src/trap.rs`
- `os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64/src/interrupt.rs`
- `os/components/wateros-platform/platform-impl/impl-qemu-loongarch64-virt/src/lib.rs`
- `os/components/wateros-platform/platform-impl/impl-qemu-loongarch64-virt/src/eiointc.rs`（新增）
- `os/components/wateros-driver/driver-impl/impl-qemu-loongarch64-virt/src/enumerate.rs`（PCI 资源/中断）
- `os/src/trap_handler.rs`

## CodeGraph 查询命令

```bash
codegraph explore "decode_loongarch64_trap_cause find_config_base probe_virtio_blk_pci"
codegraph explore "ack_interrupt PciTransport msix_config queue_msix_vector"
codegraph impact find_config_base
```

## 验收方式

- 静态：`make la_check && make rv_check`。
- QEMU smoke：LoongArch 启动、PCI virtio-blk/net 枚举、rootfs 挂载不回归。
- 确认 PCI INTx（或 MSI-X）中断链路可被虚拟中断桩触发并 claim/complete。

## 验收命令

```bash
cd /home/zhitian/project/WaterOS_irq_linux_style/os
make la_check && make rv_check
make kernel-la-final
make la_qemu_run_snapshot
```

## 任务简报

完成后写 `history/03-loongarch-eiointc-pci.md`。
