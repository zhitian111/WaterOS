# 23 SD 只读首扇区：1-bit / 25MHz 默认模式

## 任务内容

修复 SD host 注册失败：`register: Read(IoError)`。

进展：任务 22 的 400kHz 识别已生效（识别/CSD/RCA/select 全通过），失败点在
**识别后以 4-bit / 50MHz 读首扇区**。

根因：SD 卡上电默认是 1-bit、Default Speed（≤25MHz）。我们的驱动没有发送
ACMD6（SET_BUS_WIDTH 4-bit）与 CMD6（switch 到 High Speed 50MHz），却把
控制器配成 4-bit / 50MHz——数据线宽度与卡侧不匹配（卡只在 DAT0 输出，
控制器按 DAT0-3 采样），且时钟超过卡当前模式上限，单块读必然数据错误。
U-Boot 能 4-bit/50MHz 是因为它按规范发过这两条命令。

## 实施方案

1. `activate_and_register_readonly` 的识别计划同时把 `bus_width` 降为 1：
   - 识别 1-bit / 400kHz（现状保持）；
   - 识别完成后提升时钟到 **25MHz**（卡默认模式上限，无需 CMD6）；
   - 保持 1-bit（控制器 CTYPE=0，与卡默认一致，无需 ACMD6）。
2. `SD_TRANSFER_HZ` 从 50M 改为 25M，注释注明 ACMD6/CMD6 未实现，
   高速/宽总线作为后续性能任务。
3. eMMC host（无卡）失败日志保持预期噪音。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-driver/driver-impl/impl-jh7110-visionfive2/src/mmc.rs`

CodeGraph：

```bash
codegraph explore "activate_and_register_readonly"
codegraph explore "read_single_block"
```

## 验收方式

- [ ] `cargo test -p wateros-driver-impl-jh7110-visionfive2` 通过。
- [ ] `make jh7110_check` / `make rv_check` 通过。
- [ ] 真机 SD host 注册成功（`registered block device #...`），fs 探测到
      `/dev/vda4`。

## 验收命令

```bash
cd os/components/wateros-driver
cargo test -p wateros-driver-impl-jh7110-visionfive2
cd ../../../os
make jh7110_check && make rv_check
make jh7110_uimage && make jh7110_bootdir
cd ../user && make disk ARCH=rv PACKAGE=minimal IMAGE_SIZE_MB=64 \
  DISK_SIZE_MB=192 BOOT_DIR=../os/build/jh7110-boot BOOT_SIZE_MB=64
git diff --check
```

## 验证环境

- L0 宿主机：单测/check。✅
- L3 真机：1-bit/25MHz 首扇区读。🔴（本次已复现 Read(IoError)）

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - `impl-jh7110-visionfive2/mmc.rs`：识别计划同时设 `bus_width=1`；
    `SD_TRANSFER_HZ` 50M → 25M（卡默认模式上限）；注释说明 ACMD6/CMD6
    未实现，4-bit/High Speed 留作后续性能任务。
- 验收结果：
  - `cargo test -p wateros-driver-impl-jh7110-visionfive2`：18 passed。
  - `make jh7110_check` / `make rv_check`：通过。
  - `make jh7110_uimage` / `jh7110_bootdir` / `make disk`：镜像重建。
  - `git diff --check`：clean。
- 真机验证（待用户重烧）：
  - 预期 SD host `registered block device #...`，fs 探测到 `/dev/vda4`；
  - 若 1-bit/25MHz 仍读失败，说明单块读数据通路（FIFO/状态机）需要单独
    排查，下一步加寄存器级诊断日志；
  - 成功后进入写路径任务（`write_blocks` 当前返回 Unsupported）。
