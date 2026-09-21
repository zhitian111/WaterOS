# 30 SD PIO 稳定化：降传输时钟 + 每次读前整机复位

## 任务内容

稳定连续多块读，消除 `read_single_block ... err=Fifo rintsts=0x808`
（bit11 FRUN）在连续读第 4 块出现的溢出。

真机/DTB 证据：JH7110 SD host `fifo-depth=0x20`（32 字，仅 128 字节），且
无 `fifo-mode`——U-Boot 对该控制器走 IDMAC（DMA），PIO 路径非本板主线。
512B 块在 25MHz 下需流经 4 倍于 FIFO 深度的数据，排空稍慢即溢出。

## 实施方案

1. `SD_TRANSFER_HZ` 25MHz → 1MHz（FIFO 缓冲时间约 60 倍余量），保证 PIO
   可靠；吞吐降低，作为 rootfs 读通的临时方案。
2. `read_single_block` 每次读前用整机复位（FIFO+CMD+DATA，`CTRL_RESET_ALL`）
   替代仅 FIFO 复位，清除残留状态。
3. 记录待办：正确解法为 IDMAC + Zicbom 缓存维护，另立后续任务。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-driver/driver-impl/impl-jh7110-visionfive2/src/mmc.rs`
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
- L3 真机：1MHz PIO 连续读。🔴（本次已复现第 4 块 FRUN）

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - `impl-jh7110-visionfive2/mmc.rs`：`SD_TRANSFER_HZ` 25MHz → 1MHz，
    注释说明 FIFO 32 字、IDMAC 为正确解。
  - `impl-dw-mmc/mmc.rs`：`read_single_block` 每次读前改用整机复位
    `reset()`（FIFO+CMD+DATA），删除仅 FIFO 复位的 `reset_fifo` 与
    未使用的 `CTRL_FIFO_RESET` 常量。
- 验收结果：
  - `cargo test -p wateros-driver-block-impl-dw-mmc`：12 passed。
  - `cargo test -p wateros-driver-impl-jh7110-visionfive2`：18 passed。
  - `make jh7110_check` / `make rv_check`：通过。
  - `make jh7110_uimage` / `jh7110_bootdir` / `make disk`：镜像重建。
  - `git diff --check`：clean。
- 真机验证（待用户重烧）：
  - 预期 1MHz PIO 连续读稳定，分区扫描成功，`/dev/vda4` 出现并探测到 ext4；
  - 读吞吐下降是临时代价，IDMAC + Zicbom 作为后续任务恢复高速。
