# 31 SD 回退为仅 FIFO 复位 + 保留 1MHz（修命令路径超时）

## 任务内容

修复任务 30 引入的回归：每次读前整机复位（FIFO+CMD+DATA）导致
`read_single_block ... err=Timeout rintsts=0x0`（CMD17 未完成）。

结论：`reset()` 的全量复位对连续读命令路径过激进；任务 30 中真正有效的是
1MHz 慢时钟（消除 FRUN 溢出），FIFO 复位沿用任务 27 的仅 FIFO 复位即可。

## 实施方案

1. `impl-dw-mmc/mmc.rs`：恢复 `CTRL_FIFO_RESET` 与 `reset_fifo()`；
   `read_single_block` 用 `reset_fifo()` 替代 `reset()`。
2. 保留 `SD_TRANSFER_HZ = 1MHz`。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-driver/driver-block/block-impl/impl-dw-mmc/src/mmc.rs`

CodeGraph：

```bash
codegraph explore "read_single_block"
```

## 验收方式

- [ ] `cargo test -p wateros-driver-block-impl-dw-mmc` 通过。
- [ ] `cargo test -p wateros-driver-impl-jh7110-visionfive2` 通过。
- [ ] `make jh7110_check` / `make rv_check` 通过。
- [ ] 真机首扇区读成功，分区扫描成功，`/dev/vda4` 出现。

## 验收命令

```bash
cd os/components/wateros-driver
cargo test -p wateros-driver-block-impl-dw-mmc
cargo test -p wateros-driver-impl-jh7110-visionfive2
cd /home/zhitian/project/WaterOS_real_hardware_porting/os
make jh7110_check && make rv_check
make jh7110_uimage && make jh7110_bootdir
cd ../user && make disk ARCH=rv PACKAGE=minimal IMAGE_SIZE_MB=64 \
  DISK_SIZE_MB=192 BOOT_DIR=../os/build/jh7110-boot BOOT_SIZE_MB=64
git diff --check
```

## 验证环境

- L0 宿主机：单测/check。✅
- L3 真机：1MHz + FIFO 复位连续读。🔴

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - `impl-dw-mmc/mmc.rs`：恢复 `CTRL_FIFO_RESET` 与 `reset_fifo()`；
    `read_single_block` 用仅 FIFO 复位替换整机复位；保留 1MHz 传输时钟。
- 验收结果：
  - `cargo test -p wateros-driver-block-impl-dw-mmc`：12 passed。
  - `cargo test -p wateros-driver-impl-jh7110-visionfive2`：18 passed。
  - `make jh7110_check` / `make rv_check`：通过。
  - `make jh7110_uimage` / `jh7110_bootdir` / `make disk`：镜像重建。
  - `git diff --check`：clean。
- 真机验证（待用户重烧）：
  - 预期恢复首扇区读成功，1MHz 下连续读稳定，分区扫描通过并出现
    `/dev/vda4`。
