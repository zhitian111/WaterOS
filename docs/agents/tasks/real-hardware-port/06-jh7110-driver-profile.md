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

（完成后追加，格式见目录 README。）
