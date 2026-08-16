# 27 SD 每次读前复位 FIFO（修分区扫描 FRUN）

## 任务内容

修复 SD 分区扫描失败：`read_single_block failed err=Fifo rintsts=0x808`
（bit11 = FRUN，FIFO 溢出）。

进展：注册探测读已成功，SD 整盘设备已注册（`registered block device #0`）；
失败点转为 `block::register_block_device` 的分区扫描再读扇区时的 FIFO 溢出。

根因：同板 U-Boot 的 PIO 路径在每次数据传输前执行
`dwmci_wait_reset(DWMCI_CTRL_FIFO_RESET)`（CTRL bit1），我们的驱动仅在
初始化时复位一次；连续第二次读带着上次 FIFO 状态，溢出（FRUN）。

## 实施方案

1. `impl-dw-mmc/mmc.rs`：新增 `CTRL_FIFO_RESET = 1 << 1` 与
   `DwMmc::reset_fifo()`（写 bit1 并等待清零）。
2. `read_single_block` 开头调用 `reset_fifo()`。

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
- [ ] 真机分区扫描成功，devfs 出现 `/dev/vda4`，fs 探测到 ext4。

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
- L3 真机：连续多扇区读。🔴（本次已复现第二次读 FRUN）

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - `impl-dw-mmc/mmc.rs`：新增 `CTRL_FIFO_RESET`（bit1）与
    `DwMmc::reset_fifo()`；`read_single_block` 开头先复位 FIFO。
- 验收结果：
  - `cargo test -p wateros-driver-block-impl-dw-mmc`：12 passed。
  - `cargo test -p wateros-driver-impl-jh7110-visionfive2`：18 passed。
  - `make jh7110_check` / `make rv_check`：通过。
  - `make jh7110_uimage` / `jh7110_bootdir` / `make disk`：镜像重建。
  - `git diff --check`：clean。
- 真机验证（待用户重烧）：
  - 预期分区扫描成功，devfs 出现 `/dev/vda1..4`，fs 探测到 `/dev/vda4`
    并进入挂载；随后是写路径（`write_blocks` 当前 Unsupported）。
