# 21 JH7110 MMC 激活与 SD 只读枚举（真机首轮解锁）

## 任务内容

把 `MmcBringUpPlan` 从“只打印 plan”改为**实际激活**：满足真机证据后调用
`initialize_controller → initialize_sd_card → register_readonly_block_device`，
让 SD 卡以只读块设备出现在 devfs（预期 `/dev/vda` + 分区），为 rootfs
挂载铺路。这是任务 08b 真机验收的第一阶段。

## 实施方案

1. `impl-jh7110-visionfive2/src/mmc.rs`：
   - 新增 `MMC_ACTIVATION_EVIDENCE`（四项全 true，附证据说明：DTB 拓扑
     与 PLIC S 态上下文已真机确认；控制器时钟/reset/pinmux 由 U-Boot 保持
     打开，曾以 50MHz 读同一张卡）；
   - 新增 `MMC_INPUT_FREQUENCY_HZ = 100_000_000`（JH7110 CIU 时钟，分频
     后目标 50MHz，与 U-Boot SD High Speed 一致）；
   - 新增 `activate_and_register_readonly(host)`：构造 `MmioRegisters`
     → `initialize_sd_card` → `card.into_shared()` →
     `register_readonly_block_device`，错误带上下文返回。
2. `lib.rs` `Machine::init_after_boot`：对每个 MMC host，打印 plan 后尝试
   激活；成功记块设备号，失败记 error（不阻断启动，便于首轮迭代）。
3. eMMC host（无卡）预期在卡初始化处超时报错，属正常日志；SD host
   （`0x16020000`，4-bit）为目标。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-driver/driver-impl/impl-jh7110-visionfive2/src/mmc.rs`
- 同目录 `lib.rs`

CodeGraph：

```bash
codegraph explore "initialize_sd_card"
codegraph explore "register_readonly_block_device"
codegraph explore "register_block_device"
```

## 验收方式

- [ ] `make jh7110_check` / `make rv_check` 通过。
- [ ] `cargo test -p wateros-driver-impl-jh7110-visionfive2` 通过。
- [ ] 真机日志出现 SD host 激活成功（块设备注册）或明确的失败原因
      （`Core(...)`/`Card(...)`/`register: ...`），据此进入下一轮迭代。

## 验收命令

```bash
cd os
make jh7110_check
make rv_check
cargo test -p wateros-driver-impl-jh7110-visionfive2
make jh7110_uimage && make jh7110_bootdir
cd ../user && make disk ARCH=rv PACKAGE=minimal IMAGE_SIZE_MB=64 \
  DISK_SIZE_MB=192 BOOT_DIR=../os/build/jh7110-boot BOOT_SIZE_MB=64
git diff --check
```

## 验证环境

- L0 宿主机：check + crate 单测。✅
- L3 真机：首次解锁实测（时钟/分频/卡协议时序只能真机验证）。🔴

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - `impl-jh7110-visionfive2/src/mmc.rs`：新增 `MMC_ACTIVATION_EVIDENCE`
    （四项证据置 true，附 U-Boot 50MHz 读卡与 PLIC 真机使能日志依据）、
    `MMC_INPUT_FREQUENCY_HZ=100M`（分频后 50MHz）、
    `activate_and_register_readonly(host)`（`MmioRegisters` →
    `initialize_sd_card` → `into_shared` → `register_readonly_block_device`，
    错误带上下文返回）。
  - `lib.rs` `Machine::init_after_boot`：打印 plan 后对每个 host 尝试激活，
    成功记块设备号，失败记 error 不阻断启动。
- 验收结果：
  - `make jh7110_check` / `make rv_check`：通过。
  - `cargo test -p wateros-driver-impl-jh7110-visionfive2`：18 passed。
  - `make jh7110_uimage` / `jh7110_bootdir` / `make disk`：镜像重建。
  - `git diff --check`：clean。
- 真机验证（待用户重烧）：
  - 预期 SD host（`0x16020000`）激活成功，日志出现
    `MMC host base=0x16020000 activated; registered block device #...`，
    随后 `[fs] init` 探测到 `/dev/vda4`；
  - eMMC host（无卡）预计 `sd init: Card(...)` 超时报错，属预期；
  - 若 SD 也失败，错误会指明 `Core(...)`/`Card(...)` 阶段，据此调整
    输入时钟/OCR 尝试次数/分频后重试。
