# T02：RISC-V PLIC + SEIE + trap 外部中断分发

## 任务内容

在 RISC-V 上打通外部中断链路：PLIC irqchip、`sie.SEIE` 使能、trap 外部中断分发。
本任务只接入基础设施，不注册任何设备 IRQ；未注册的 IRQ 安全跳过。

## 实施方案

1. 把 `os/components/wateros-platform/platform-impl/impl-qemu-riscv64-opensbi/src/plic.rs`
   （当前未跟踪草稿）重写为 `IrqChip` 实现：
   - per-hart supervisor context、claim/complete、enable/mask、priority、set_affinity；
   - 从 DTB/平台能力推导 hart 数，不写死 `MAX_IRQS=64`，不用 `tp` 隐式假设 hart id。
2. `os/components/wateros-platform/platform-impl/impl-qemu-riscv64-opensbi/src/lib.rs` 增加 `mod plic`。
3. arch 层加外部中断使能：
   - `os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/src/interrupt.rs`（`sie.SEIE`）；
   - `os/components/wateros-platform/platform-arch/arch-api/api-v0/src/interrupt.rs`（新增 external enable 契约，避免破坏现有 `ArchTimerInterruptControl` 语义）。
4. `os/src/trap_handler.rs` 增加 `Interrupt::SupervisiorExternel` 分支：claim → 查找 `IrqAction` → 调用 top-half → complete。

## 涉及文件

- `os/components/wateros-platform/platform-impl/impl-qemu-riscv64-opensbi/src/plic.rs`
- `os/components/wateros-platform/platform-impl/impl-qemu-riscv64-opensbi/src/lib.rs`
- `os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/src/interrupt.rs`
- `os/components/wateros-platform/platform-arch/arch-api/api-v0/src/interrupt.rs`
- `os/src/trap_handler.rs`

## CodeGraph 查询命令

```bash
codegraph explore "SupervisiorExternel SupervisiorTimer SupervisiorSoft"
codegraph node wateros_kernel_trap_handler
codegraph explore "enable_timer_interrupt enable_global_interrupt clear_soft_interrupt"
codegraph impact ArchTimerInterruptControl
```

## 验收方式

- 静态：`make rv_check && make la_check`（LoongArch 保持同步回退，仅保 check）。
- QEMU smoke：RISC-V 启动、rootfs 挂载、timer/IPI 不回归；可用测试桩触发一次虚拟外部中断验证 claim/complete。
- 确认无设备 IRQ 时外部中断被安全跳过，不 panic。

## 验收命令

```bash
cd /home/zhitian/project/WaterOS_irq_linux_style/os
make rv_check && make la_check
make kernel-rv-final
make rv_qemu_run_snapshot
```

## 任务简报

完成后写 `history/02-riscv-plic-seie-trap.md`。
