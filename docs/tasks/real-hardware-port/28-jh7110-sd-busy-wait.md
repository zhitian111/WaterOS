# 28 SD 读前等待 DATA_BUSY 清空（修分区扫描 FRUN）

## 任务内容

修复分区扫描读 GPT 头时的 FRUN（`rintsts=0x808`，bit11 FIFO 溢出）。

对照 U-Boot `dwmci_send_cmd`：每次命令/数据传输前先
`while (STATUS & DWMCI_BUSY)` 等待数据引擎空闲；我们的 `read_single_block`
缺少这一步。第一次读（注册探测，无前置数据）因此成功，第二次读（分区扫描
GPT 头，紧跟上次数据）在控制器仍 BUSY 时发 CMD17 → FRUN。

## 实施方案

1. `impl-dw-mmc/mmc.rs`：新增 `STATUS_BUSY = 1 << 9` 与
   `DwMmc::wait_not_busy()`。
2. `read_single_block` 开头在 `reset_fifo` 前先 `wait_not_busy()`。
3. `read_failure` 额外打印 `CMDARG`（失败地址），便于后续定位具体扇区。

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
- [ ] 真机分区扫描成功，devfs 出现 `/dev/vda4`。

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
- L3 真机：连续读扇区。🔴（本次已复现第二次读 FRUN）

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - `impl-dw-mmc/mmc.rs`：新增 `STATUS_BUSY (1<<9)` 与
    `wait_not_busy()`；`read_single_block` 先等待 busy 清零再复位 FIFO；
    `read_failure` 增加 `CMDARG`（失败地址）打印。
- 验收结果：
  - `cargo test -p wateros-driver-block-impl-dw-mmc`：12 passed。
  - `cargo test -p wateros-driver-impl-jh7110-visionfive2`：18 passed。
  - `make jh7110_check` / `make rv_check`：通过。
  - `make jh7110_uimage` / `jh7110_bootdir` / `make disk`：镜像重建。
  - `git diff --check`：clean。
- 真机验证（待用户重烧）：
  - 预期分区扫描成功，devfs 出现 `/dev/vda4`；
  - 若仍失败，`cmdarg=` 会给出具体扇区地址，进一步缩小范围。
