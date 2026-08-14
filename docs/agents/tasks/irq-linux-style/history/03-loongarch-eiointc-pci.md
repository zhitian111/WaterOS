# T03 任务简报：LoongArch EIOINTC + PCI interrupt-map + ESTAT/ECFG

- 完成日期：2026-08-15
- commit：`407b9225`（`[feat] 接入 LoongArch EIOINTC 外部中断链路（ECFG + ESTAT 解码）`）
- 前置：`7dd4046f`（T02 RISC-V PLIC 链路）

## 实际改动摘要

架构层（`wateros-platform-arch`）：

- `arch-impl/impl-loongarch64/src/interrupt.rs`：新增 `ECFG_EXTERNAL_INTERRUPT_ENABLE`
  （`1 << 5`，QEMU virt 把 EIOINTC 接到 CPU 硬件中断输入 3）与
  `enable_external_interrupt` / `disable_external_interrupt`。
- `arch-impl/impl-loongarch64/src/trap.rs`：新增 `EXTERNAL_INTERRUPT_PENDING`
  （`1 << 5`），`decode_loongarch64_trap_cause` 在 IPI/timer 之后解码为
  `Interrupt::SupervisiorExternel`。
- `platform-arch/src/lib.rs` 的 `pub mod interrupt`：补 LoongArch 外部中断开关
  （cfg `impl-loongarch64`），与 RISC-V 并列。

平台层：

- 新增 `platform-impl/impl-qemu-loongarch64-virt/src/external_irq.rs`：EIOINTC
  （IOCSR：ENABLE `0x1600` / ISR `0x1800` / ROUTE `0x1c00`）+ PCH-PIC（MMIO
  `0x1000_0000`，MASK `0x20` / CLR `0x80`）；`init_current_cpu` 使能全部向量、
  把 PCI INTx 16..19 路由到当前 CPU、清 PCH mask/pending；`set_enabled` 按向量
  改 EIO_ENABLE；`claim` 扫 ISR；`complete` 仅对 PCI INTx 清 PCH-PIC。
- `wateros-platform/src/external_irq.rs` 改为按 board profile 选择 `active` 实现
  （PLIC / EIOINTC），公共门面 `init_current_cpu` 同时打开架构外部中断使能。
- `wateros-platform/src/lib.rs`：`pub mod external_irq` 的 cfg 改为 rv 或 la。

启动与 trap：

- `os/src/main.rs` LoongArch BSP/AP 在全局中断打开前调用
  `platform::external_irq::init_current_cpu()`。
- `os/src/trap_handler.rs` 外部中断分发分支 cfg 改为 rv 或 la。

## 验证命令与结果

- `cd os && make rv_check`：通过。
- `cd os && make la_check`：通过（含 EIOINTC/ESTAT 解码编译）。
- `make kernel-la-final`：构建成功（8.92s）。
- `git diff --check`：干净。

## 未验证项 / 风险 / 下一步

- **LoongArch QEMU 运行 smoke 未执行**：`~/Downloads/sdcard-la-pub.img.gz` 解压后
  约 14GB，当前根分区仅约 12GB 可用，磁盘空间不足；待空间释放/镜像就绪后补
  `qemu-system-loongarch64`（QEMU 9.2.1 + `-snapshot`）启动验证。
- EIOINTC 配置（向量使能、PCI INTx 16..19 路由、PCH-PIC 清掩码）沿用历史实验
  （`8abc00b0`）已验证的寄存器布局；本轮仅编译验证，未上机读回确认。
- PCI `interrupt-map` 的 DTB 语义解码仍待 T09（LoongArch 块异步）按需实现；
  当前 EIOINTC 向量号按 QEMU virt 约定直接使用。
- 下一步 T04：bottom-half / softirq + irqaction 线程化（ISR 只 ack，heavy 回收
  与唤醒放可调度上下文）。
