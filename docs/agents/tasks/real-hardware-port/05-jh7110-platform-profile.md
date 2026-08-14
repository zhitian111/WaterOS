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

（完成后追加，格式见目录 README。）
