# 32 SD 写路径：CMD24 + FIFO 写入（rootfs RW 最后一关）

## 任务内容

实现 DW MMC SD 单块写，消除真机 root-layout 的
`write_blocks → Unsupported`，打通 ext4 RW 持久化闭环。

前提已达成：SD 读、分区扫描、rootfs 挂载（`ext4 root mounted (RW)`）均
真机通过；失败点只剩写路径。

## 实施方案

1. `impl-dw-mmc/mmc.rs`：
   - 新增 `CMD_WRITE = 1 << 10`、`STATUS_FIFO_FULL = 1 << 3`；
   - 新增 `write_single_block`：复位/配 BLKSIZ/BYTCNT/CMDARG → 发 CMD24
     （`CMD_DATA_EXPECTED | CMD_WRITE | ...`）→ 按 `STATUS.FIFO_FULL`
     背压逐字写 FIFO → 等 DTO；
   - 诊断函数 `read_failure` 更名 `data_failure`，日志改
     `[dw-mmc] data transfer failed`。
2. `impl-dw-mmc/sd.rs`：
   - `SdTransport` 增加 `write_single_block`；
   - `SdCard::write_blocks` 实现逐块写（地址/边界与读一致）；
   - `flush` 注释更新（PIO 直写无缓冲）。
3. 测试更新：`ScriptedCard` 支持写并记录地址；`write_blocks` 由断言
   Unsupported 改为断言成功。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-driver/driver-block/block-impl/impl-dw-mmc/src/mmc.rs`
- `os/components/wateros-driver/driver-block/block-impl/impl-dw-mmc/src/sd.rs`

CodeGraph：

```bash
codegraph explore "write_single_block"
codegraph explore "write_blocks"
```

## 验收方式

- [ ] `cargo test -p wateros-driver-block-impl-dw-mmc` 通过。
- [ ] `cargo test -p wateros-driver-impl-jh7110-visionfive2` 通过。
- [ ] `make jh7110_check` / `make rv_check` 通过。
- [ ] 真机 root-layout 写入成功，`/sbin/init` 可探测，进入 userspace-init
      或 busybox；重开镜像持久化一致性由宿主机 `e2fsck -fn` 复核。

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
- L3 真机：1MHz PIO 写首扇区 + root-layout。🔴（读/挂载已通过）

## 任务简报

（任务完成后填写：完成日期、commit、实际改动、验收结果、未验证项）
