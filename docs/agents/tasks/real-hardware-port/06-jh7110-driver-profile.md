# 06 JH7110 驱动 profile（UART/PLIC/IRQ 拓扑）

## 任务内容

从 `feat/visionfive2-port` 迁移 `impl-jh7110-visionfive2` 驱动实现中的
`topology/plic/irq/uart`（约 1412 行整体中最核心、且有 host 单测的部分），并复用
`impl-uart-16550` 的 `DwApb32` 布局。保持原分支的 fail-closed 门控
（`MmcHardwareEvidence`/`MmcActivationBlocker` 风格），不宣称硬件已完成。

本任务完成外部中断与串口在 JH7110 上的软件就绪；真实 MMIO/时序仍后置。

## 实施方案

1. 迁移 `plic.rs`（RISC-V PLIC 驱动，对照 `ax-riscv-plic`/`plic` crate 评估是否需要替代）、
   `irq.rs`、`topology.rs`、`uart.rs`、`lib.rs`。
2. UART 改用 `impl-uart-16550`（`DwApb32`）注册，平台层只传基址。
3. 接入任务 01 的 `handle_external_interrupt`（PLIC claim/complete）。
4. 迁移并跑通原分支的 UART/PLIC/topology host 单测。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-driver/driver-impl/impl-jh7110-visionfive2/**`（新增）
- `os/components/wateros-driver/driver-character/character-impl/impl-uart-16550/**`
- `os/components/wateros-driver/src/lib.rs`、`Cargo.toml`

CodeGraph：

```bash
codegraph explore "handle_external_interrupt"
codegraph explore "register_character_device"
codegraph explore "plic"
```

## 验收方式

- [ ] UART/PLIC/IRQ 拓扑 host 单测通过。
- [ ] `--features jh7110-visionfive2,pre` 能编译。
- [ ] 外部中断 claim/complete 语义在单测中被覆盖（无泄漏/重复注销）。

## 验收命令

```bash
cd os
make configure
make rv_check
cargo test -p wateros-driver-impl-jh7110-visionfive2   # 以实际 package 名为准
git diff --check
```

## 验证环境

- L0 宿主机：驱动状态机/PLIC 单测。✅
- L1 QEMU virt：RISC-V PLIC 契约可在 QEMU virt 上对照验证（地址不同，逻辑相同）。🟡
- L3 真机：JH7110 实际 PLIC/IRQ 时序需真机。🔴

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - 新增 `driver-impl/impl-jh7110-visionfive2`（迁移自 `feat/visionfive2-port` 并审计）：
    `topology.rs`（DTB 发现 console UART/PLIC/MMC host）、`plic.rs`（PLIC 寄存器算术 +
    MMIO 访问）、`irq.rs`（claim/complete、`IrqLease` 生命周期、in-flight drain）、
    `uart.rs`（reg-shift/reg-io-width → `RegisterLayout`）、`lib.rs`
    （`MachineDriver`：`init_after_boot`/`handle_external_interrupt`/`init_current_cpu`）。
  - `mmc.rs`：任务 06 只迁移资源类型与 `bring_up_plan`（fail-closed 门控，
    `can_activate()==false`）；DW MMC 控制器/SD 协议在任务 07 接入。
  - arch 补 `sie.SEIE` 使能：`impl-riscv64/src/interrupt.rs` 增加
    `enable_external_interrupt`/`disable_external_interrupt`，经 platform-arch 门面
    再导出（LoongArch 分支留任务 11）。
  - 接线：`wateros-driver` workspace 成员/依赖/feature `impl-jh7110-visionfive2`、
    `machine()` 分支、uart 再导出；顶层 `jh7110-visionfive2` feature 改为启用真实
    驱动。
  - PLIC 轮子评估：对照 `ax-riscv-plic`/`plic` crate，保留迁移实现（自包含、有
    单测、与 topology/lease 模型集成；替代 crate 的注册模型需额外适配），在简报
    中记录结论。
- 验收结果：
  - `cargo test -p wateros-driver-impl-jh7110-visionfive2`：10 passed（host：
    uart 1 + plic 4 + topology 2 + irq 3）。
  - `cargo check -p wateros-driver-impl-jh7110-visionfive2
    --target riscv64gc-unknown-none-elf`：通过。
  - `cargo check --no-default-features --features jh7110-visionfive2,pre
    --target riscv64gc-unknown-none-elf`：通过。
  - `make rv_check`、`make la_check`：无回归。
  - `git diff --check`：clean。
- 未验证/风险：
  - 真机未验证：PLIC 实际中断时序、DW APB UART 线参数、MMC 时钟/reset/pinmux；
    所有硬件激活保持 fail-closed（`HardwareEvidence` 门控）。
  - `irq.rs` 的 `initialize_current_hart` 依赖 platform 恒等映射 PLIC 窗口与
    `sie.SEIE`；尚未在 QEMU virt 上对照验证（QEMU virt PLIC 地址不同，逻辑相同）。
