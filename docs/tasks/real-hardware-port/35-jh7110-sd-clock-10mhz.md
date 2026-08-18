# 35 SD PIO 提速：传输时钟 1MHz → 10MHz

## 任务内容

缓解 SD 读速过慢：`SD_TRANSFER_HZ` 从 1MHz 提到 10MHz。

背景：1MHz 是任务 30 的保守保底；25MHz 曾出现间歇 FRUN（第 4 块），
10MHz 保留约 4 倍余量，预期稳定且约 8 倍读速提升。U74 无 `zicbom`，
IDMAC/DMA 缓存一致性是后续大任务，本任务只做 PIO 时钟档位调整。

## 实施方案

1. `impl-jh7110-visionfive2/mmc.rs`：`SD_TRANSFER_HZ` 1MHz → 10MHz，
   注释保留 25MHz/IDMAC 说明。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-driver/driver-impl/impl-jh7110-visionfive2/src/mmc.rs`

CodeGraph：

```bash
codegraph explore "SD_TRANSFER_HZ"
```

## 验收方式

- [ ] `make jh7110_check` / `make rv_check` 通过。
- [ ] 真机连续读写无 FRUN，登录 shell 命令明显变快。

## 验收命令

```bash
cd os
make jh7110_check && make rv_check
make jh7110_uimage && make jh7110_bootdir
cd ../user && make disk ARCH=rv PACKAGE=minimal IMAGE_SIZE_MB=64 \
  DISK_SIZE_MB=192 BOOT_DIR=../os/build/jh7110-boot BOOT_SIZE_MB=64
git diff --check
```

## 验证环境

- L0 宿主机：check。✅
- L3 真机：10MHz PIO 连续读。🔴

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - `impl-jh7110-visionfive2/mmc.rs`：`SD_TRANSFER_HZ` 1MHz → 10MHz，
    注释补充 FRUN/IDMAC 背景。
- 验收结果：
  - `make jh7110_check` / `make rv_check`：通过。
  - `make jh7110_uimage` / `jh7110_bootdir` / `make disk`：镜像重建。
  - `git diff --check`：clean。
- 真机验证（待用户重烧）：
  - 预期 10MHz PIO 连续读写稳定、命令明显变快；若仍 FRUN 则降档并转
    IDMAC 专项。
