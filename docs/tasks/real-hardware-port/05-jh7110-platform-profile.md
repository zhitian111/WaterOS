# 05 JH7110 平台 profile

## 任务内容

从 `feat/visionfive2-port` 选择性迁移 `impl-jh7110-visionfive2` 平台实现（约 175 行：
`boot/console/dtb/memory/time`，reset/smp/timer 复用 `impl-opensbi-common`），为
VisionFive 2 / JH7110 建立第一个可编译的 RISC-V 板级平台 profile。

这是三块旧工作树里最收敛、最值得优先迁移的部分，需按当前 main 审计后重新接合。

## 实施方案

1. 迁移 `impl-opensbi-common`（reset/smp/timer 的 OpenSBI transport）。
2. 迁移 `impl-jh7110-visionfive2` 的 `boot.rs`（DTB 定位）、`memory.rs`（DTB 推导 RAM/
   MMIO/probe 布局）、`console.rs`（DW APB UART，复用 `impl-uart-16550` 的 `DwApb32`）、
   `time.rs`（OpenSBI timebase）。
3. 接入任务 00 的 `memory()` 契约与任务 01 的驱动接口（stub/最小实现）。
4. 为 DTB/内存解析补 host 单测（用固定 JH7110 DTB fixture）。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-platform/platform-impl/impl-opensbi-common/**`（新增）
- `os/components/wateros-platform/platform-impl/impl-jh7110-visionfive2/**`（新增）
- `os/components/wateros-platform/src/lib.rs`、`Cargo.toml`

CodeGraph：

```bash
codegraph explore "device_tree_phys_addr"
codegraph explore "boot"
codegraph explore "console"
codegraph explore "memory"
```

## 验收方式

- [ ] `--features jh7110-visionfive2,pre` 能 `cargo check` 通过（RISC-V target）。
- [ ] DTB/内存布局解析有 host 单测且通过。
- [ ] 不引入对 QEMU RISC-V 默认 profile 的回归。

## 验收命令

```bash
cd os
make configure
make rv_check
cargo check --no-default-features --features jh7110-visionfive2,pre
cargo test -p wateros-platform-impl-jh7110-visionfive2   # 以实际 package 名为准
git diff --check
```

## 验证环境

- L0 宿主机：`cargo check` + 单测。✅
- L1 QEMU virt：用 QEMU RISC-V virt 对照验证 OpenSBI timebase/console 契约的通用路径。🟡
- L2 板级 QEMU fork：若引入 JH7110 QEMU fork 可做启动冒烟。🟡
- L3 真机：真实 DRAM/时钟/PLL/UART 时序需真机（后置到 08 联调）。🔴

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - 新增 `impl-opensbi-common`：板级无关 OpenSBI 运输层（reset/smp/timer）；
    `smp.rs` 按当前 main 的 `PlatformSmp` trait 补齐 `flush_icache_remote`
    （`sbi::remote_fence_i`）。
  - 替换任务 04 的 `impl-jh7110-visionfive2` 占位为真实实现（迁移自
    `feat/visionfive2-port` 并审计）：`boot.rs`（a0=hart id/a1=DTB）、`dtb.rs`、
    `console.rs`（DW APB UART0 @ 0x1000_0000，32 位 MMIO）、`memory.rs`
    （RAM 0x4000_0000 起、DTB 推导上界、fallback 0x8000_0000）、`time.rs`
    （OpenSBI 4 MHz ACLINT fallback）、`asm/_start.S`、`linker/link.ld`；
    reset/smp/timer 复用 `impl-opensbi-common`。
  - `wateros-platform` workspace 增加 `impl-opensbi-common` 成员。
  - 为 memory 补 host 单测：`fallback_layout_is_valid` + `ram_end_falls_back_without_dtb`
    （2 个通过）。
  - 同步 `components/wateros-platform/README.md`。
- 验收结果：
  - `cargo test -p wateros-platform-impl-jh7110-visionfive2`：2 passed（host）。
  - `cargo check -p wateros-platform-impl-jh7110-visionfive2
    --target riscv64gc-unknown-none-elf`：通过。
  - `cargo check --no-default-features --features jh7110-visionfive2,pre
    --target riscv64gc-unknown-none-elf`：通过。
  - `make rv_check`（默认 QEMU）：无回归。
  - `git diff --check`：clean。
- 未验证/风险：
  - 真机未验证：UART/内存/时钟时序需任务 08 真机联调；`_start.S` 的
    `__wateros_arch_boot` 入口与 `link.ld` 的 `KERNEL_ENTRY_ADDRESS=0x40200000`
    尚未对照实际 U-Boot 环境确认（保留 TODO）。
  - 未做 `src/main.rs` 板级 bring-up 模块（check 级验收范围）；内核链接/启动
    待任务 06/08。
