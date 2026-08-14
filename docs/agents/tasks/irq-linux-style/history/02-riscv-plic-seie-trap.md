# T02 任务简报：RISC-V PLIC + SEIE + trap 外部中断分发

- 完成日期：2026-08-15
- commit：`7dd4046f`（`[feat] 接入 RISC-V PLIC 外部中断链路（SEIE + trap 分发）`）
- 前置：`5a3af15a`（T01 IRQ 核心 + DTB 解析）

## 实际改动摘要

IRQ 核心（`wateros-irq`）：

- `domain::line_by_hwirq`：hwirq 反查 virq（分发路径用）。
- `action::dispatch`：按 virq 取首个匹配 action 并调用 handler；锁释放后调用，
  避免 handler 内注册操作自死锁；注释说明注册表 spin 锁需在真实设备 IRQ 启用前
  改为中断安全（T06）。

MM 映射：

- `base-config/mm.rs` 新增 `QEMU_VIRT_PLIC_PHYS_START/END`（`0x0c00_0000..0x0c60_0000`）。
- `kernel_global.rs` 对 PLIC aperture 做 R|W 恒等映射（历史实验曾因缺映射触发
  `0xc20b000` StorePageFault，本轮直接补上）。

架构层：

- `arch-impl/impl-riscv64/src/interrupt.rs` 新增 `enable_external_interrupt` /
  `disable_external_interrupt`（`sie.SEIE`）。
- `platform-arch/src/lib.rs` 的 `pub mod interrupt` 暴露 cfg-gated 的外部中断开关
  （T03 再补 LoongArch）。

平台层：

- `platform-impl/.../plic.rs`：PLIC S-mode irqchip 与 claim/complete 原语：
  priority/pending/enable/context 寄存器、`supervisor_context(cpu)=2*cpu+1`、
  `init_current_cpu`（阈值清零）、`set_enabled/is_enabled/is_pending`、
  `PlicChip`（实现 `IrqChip`，绑定目标 CPU 上下文）。
- `wateros-platform/src/external_irq.rs`：平台外部中断门面（`init_current_cpu` 同时
  打开 SEIE；`claim/complete/set_enabled`；`dispatch_external` 循环 claim→查找
  action→complete，未处理中断 warn 后照常 complete）。

启动与 trap：

- `os/src/main.rs` RISC-V BSP/AP 在页表就绪、全局中断打开前调用
  `platform::external_irq::init_current_cpu()`。
- `os/src/trap_handler.rs` 增加 `SupervisiorExternel` 分支（cfg rv）→
  `platform::external_irq::dispatch_external()`。

## 验证命令与结果

- `cd os/components/wateros-irq && cargo test`：1 passed。
- `cd os && make rv_check`：通过。
- `cd os && make la_check`：通过（LoongArch 保持同步回退，外部中断分支 cfg 隔离）。
- `make kernel-rv-final`：构建成功。
- RISC-V QEMU smoke（QEMU 9.2.1 + `-snapshot`，`sdcard-rv-pub.img`，8 核 16G）：
  - 驱动枚举、`[fs] init end`、rootfs RW 挂载、cagent_testcode.sh 成功
    （`exit_code=0`），进入 buildstorm_testcode.sh；
  - 全程无 `panic` / fatal / page fault / `[irq] unhandled`。

## 未验证项 / 风险 / 下一步

- 尚未注册任何设备 IRQ（PLIC enable 位全 0），外部中断分发路径只被「安全跳过」
  逻辑覆盖，未做真实设备中断触发验证（属 T06 块异步）。
- 注册表 `spin` 锁尚未中断安全化；启用真实设备 IRQ 前（T06）必须处理，否则同一
  CPU 在注册期间被外部中断打断会自旋死锁（已在 `action::dispatch` 注释标注）。
- LoongArch EIOINTC/ESTAT 外部中断解码未做（T03）。
- 下一步 T03：LoongArch EIOINTC + PCI interrupt-map + ESTAT/ECFG。
