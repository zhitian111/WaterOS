# T01 任务简报：IRQ 核心抽象 + DTB irq-domain 解析

- 完成日期：2026-08-15
- commit：`5a3af15a`（`[feat] 新增 IRQ 核心抽象与 DTB irq-domain 解析`）
- 前置：`e09e006e`（任务计划文档）

## 实际改动摘要

新增 `os/components/wateros-irq/` 独立 crate（自研，不依赖 `irq-framework`）：

- `types.rs`：`Virq` / `HwIrq` / `IrqTrigger` / `IrqAffinity` / `IrqError`。
- `chip.rs`：`IrqChip` trait（enable/disable/mask/unmask/ack/eoi/set_affinity）。
- `domain.rs`：`(chip, hwirq) -> virq` 运行期注册与查询。
- `action.rs`：`IrqHandler = fn(Virq, usize)` 注册/释放/查询（top-half 绑定，供 T02 分发）。
- `lib.rs`：`self_test`（feature 门控）+ 宿主单元测试。

DTB 解析升级（`wateros-driver`）：

- `driver-api` 新增 `IrqSpec { parent, cells }`，`DeviceInfo` 增加 `irqs: Vec<IrqSpec>`，
  保留 `irq: Option<IrqLine>` 向后兼容。
- `impl-common/dtb.rs` 新增 `interrupt_parent_phandle`、`interrupt_cells_of`、
  `parse_irq_specs`（按 `#interrupt-cells` 解析 `interrupts` 多行），以及
  `interrupt_map_raw` / `interrupt_map_mask` 原始读取（语义解码留待 T03）。
- RISC-V 枚举路径填充 `irqs`，devfs 诊断日志输出 `irqs`。

接线：

- `wateros-platform` 依赖并 `pub use irq`，`self_test` feature 传播到 `irq/self_test`。
- `os/Cargo.lock` 增量更新。

## 验证命令与结果

- `cd os/components/wateros-irq && cargo test`：1 passed。
- `cargo check -p wateros-driver-api-v0 -p wateros-driver-impl-common`：通过。
- `cd os && make rv_check`：通过（对 riscv 平台 cargo check 完成）。
- `cd os && make la_check`：通过（对 loongarch64 平台 cargo check 完成）。
- `cargo check --features self_test`（wateros-irq）：通过。
- `git diff --check`：干净。

## 未验证项 / 风险 / 下一步

- 未在真机 DTB 上验证 `#interrupt-cells` 读取（QEMU virt 的 virtio 节点显式带
  `interrupt-parent`；继承语义未实现，已记录在代码注释与 T03 计划）。
- `interrupt-map` 的语义解码尚未实现，属 T03（LoongArch PCI）范围。
- 下一步 T02：RISC-V PLIC irqchip + `sie.SEIE` + trap 外部中断分发。
