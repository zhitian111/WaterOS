# 07 DW MMC 块设备接入（SD 只读）

## 任务内容

从 `feat/real-hardware-common`/`feat/visionfive2-port` 迁移共享 `impl-dw-mmc`
（`block-impl/impl-dw-mmc`，含 `mmc.rs`/`sd.rs`），接入 WaterOS block API，先支持
JH7110 的 SD 只读路径。

优先评估现成轮子 `dwmmc-host`（+ `sdmmc-protocol`）能否替代/收窄手写状态机；不满足
则保留迁移来的实现并补单测。JH7110 的 tuning/DLL 路径如轮子不支持，先 fail-closed。

## 实施方案

1. 迁移 `impl-dw-mmc` 的 `mmc.rs`/`sd.rs`，对齐 `driver-block` 的 `BlockDevice` 契约。
2. 在 `impl-jh7110-visionfive2` 驱动里接 `mmc.rs`（bus-width/时钟/reset 门控）。
3. 评估并决定是否引入 `dwmmc-host`/`sdmmc-protocol`；若引入，记录 license 与适配点。
4. 补命令序列/状态机 host 单测（无真硬件时以 fixture 驱动状态跳转）。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-driver/driver-block/block-impl/impl-dw-mmc/**`（新增）
- `os/components/wateros-driver/driver-block/src/lib.rs`
- `os/components/wateros-driver/driver-impl/impl-jh7110-visionfive2/src/mmc.rs`

CodeGraph：

```bash
codegraph explore "BlockDevice"
codegraph explore "read_block"
codegraph explore "register_block_device"
```

## 验收方式

- [ ] DW MMC 命令/状态机 host 单测通过。
- [ ] block 后端能通过 facade 编译并注册。
- [ ] SD 只读在真机至少能枚举/读一个扇区（真机项，可延后到 08 一并验收）。

## 验收命令

```bash
cd os
make configure
make rv_check
cargo test -p wateros-driver-block-impl-dw-mmc   # 以实际 package 名为准
git diff --check
```

## 验证环境

- L0 宿主机：状态机单测。✅
- L2 板级 QEMU fork：JH7110 MMC 仿真（若有）。🟠
- L3 真机：真实 SD 枚举/读。🔴

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - 新增 `driver-block/block-impl/impl-dw-mmc`（迁移自 `feat/visionfive2-port`）：
    `mmc.rs`（DW MSHC PIO/polling 原语：`RegisterIo`/`MmioRegisters`/`DwMmc`、
    `clock_divider`、probe/reset/clock/命令/单块读）、`sd.rs`（`SdTransport`/`SdCard`
    初始化协议、CSD 容量解析、`BlockDevice` 只读实现）。
  - 适配 main 的 `BlockDevice` trait：`SdCard` 与测试 fake 补 `flush`（只读 PIO
    无写缓冲，no-op）。
  - `impl-jh7110-visionfive2/src/mmc.rs` 从占位换成完整实现：`register_readonly_
    block_device`（容量/首扇区 bounded probe 后注册）、`initialize_controller`/
    `initialize_sd_card`（经 `MmcHardwareEvidence` fail-closed 门控，不注册设备）。
  - 接线：driver-block workspace 成员；驱动 Cargo.toml 增 `block`/`dw-mmc` 依赖。
- 轮子评估（dwmmc-host + sdmmc-protocol）：
  - `dwmmc-host` 提供 DW_mshc 的 SDIO host 实现，但 JH7110 的 tuning/DLL、时钟/
    reset/syscon 属板级，仍需自持；迁移实现与 `RegisterIo` 抽象已带 12 个 host
    单测且与 WaterOS `BlockDevice` 直接对齐。结论：不引入，保留迁移实现（简报记录）。
- 验收结果：
  - `cargo test -p wateros-driver-block-impl-dw-mmc`：12 passed（mmc 6 + sd 6）。
  - `cargo test -p wateros-driver-impl-jh7110-visionfive2`：17 passed（含 mmc 门控/
    plan 测试；`register_readonly_block_device` 注册后不做注销）。
  - `cargo check --no-default-features --features jh7110-visionfive2,pre
    --target riscv64gc-unknown-none-elf`：通过。
  - `make rv_check`、`make la_check`：无回归。
  - `git diff --check`：clean。
- 计划调整（基于当前 main 证据）：
  - 旧分支测试里的 `unregister_block_device` 与 `BlockDeviceRole`（Disk/Partition）
    依赖旧块注册表，main 尚无；相关断言（分区快照、注销）顺延任务 08 落地，本任务
    保留 `register_readonly_block_device` 与门控测试。
- 未验证/风险：
  - 真机 SD 枚举/读未跑（真机项，任务 08 一并验收）；时钟/reset/pinmux/卡时序
    未验证，全部激活仍 fail-closed（`HardwareEvidence` 门控）。
