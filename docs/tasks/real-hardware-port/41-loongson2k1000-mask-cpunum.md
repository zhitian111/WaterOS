# 41 Loongson 2K1000 掩码 CSR.CPUNUM

## 任务内容

早期探针只输出 `W`、未输出 `[2K1000] enter WaterOS Rust`，说明
卡在汇编 boot-stack / 跳 Rust 之间。2K1000 的 `CSR.CPUNUM` 不只
包含 core id，还有 node id 等高位；汇编此前直接用 raw CPUNUM
计算 per-CPU boot stack，可能越界。

本任务统一按 `0x1ff` 取逻辑 CPU id：

1. `_start.S` 读 `CSR.CPUNUM` 后 `andi 0x1ff`
2. `current_cpu_id()` 同样掩码 `0x1ff`

## 涉及文件

- `os/components/wateros-platform/platform-impl/impl-loongson2k1000la/src/asm/_start.S`
- `os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64/src/cpu.rs`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] 新内核已更新到 TFTP
- [ ] 板端输出从 `W` 继续到 `[2K1000] enter WaterOS Rust`

## 任务简报

- 完成日期：2026-08-16
- CPUNUM 掩码修复完成；等待板端第二轮早期探针日志。
