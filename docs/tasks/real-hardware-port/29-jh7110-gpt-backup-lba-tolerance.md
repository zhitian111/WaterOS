# 29 GPT 备份头位置容忍（小镜像写大卡）

## 任务内容

修复分区扫描 `GPT scan failed: InvalidGptHeader`。

根因：`scan_gpt_at` 要求 `backup_lba == 磁盘末扇区`。我们的 rootfs 镜像是
192 MiB（GPT 备份头在扇区 393215），而 SD 卡物理容量 29.1 GiB（约 6100 万
扇区）——小镜像写大卡后备份头不在物理磁盘末尾，被严格校验拒绝。QEMU 中
虚拟盘大小等于镜像，故不暴露。Linux/parted 对这种情况同样用主 GPT 并只
告警，不拒绝。

## 实施方案

1. `driver-block/block-api/api-v0/src/partition.rs`：
   `scan_gpt_at` 移除 `expected_backup_lba` 形参及
   `backup_lba != expected_backup_lba` 判定；保留 `backup_lba >=
   total_blocks`（备份头必须在盘内）作为边界检查。主 GPT 合法即可解析
   分区，备份头仅作损坏时的回退。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-driver/driver-block/block-api/api-v0/src/partition.rs`

CodeGraph：

```bash
codegraph explore "scan_gpt"
codegraph explore "scan_gpt_at"
```

## 验收方式

- [ ] `cargo test -p wateros-driver-block-api-v0` 通过（若存在）。
- [ ] `make jh7110_check` / `make rv_check` 通过。
- [ ] QEMU virt 回归：GPT 分区照常注册，`/dev/vda4` 挂载 login。
- [ ] 真机分区扫描成功，devfs 出现 `/dev/vda4`。

## 验收命令

```bash
cd os/components/wateros-driver
cargo test -p wateros-driver-block-api-v0 2>/dev/null || true
cd /home/zhitian/project/WaterOS_real_hardware_porting/os
make jh7110_check && make rv_check
make jh7110_uimage && make jh7110_bootdir
cd ../user && make disk ARCH=rv PACKAGE=minimal IMAGE_SIZE_MB=64 \
  DISK_SIZE_MB=192 BOOT_DIR=../os/build/jh7110-boot BOOT_SIZE_MB=64
cd ../os && make run ARCH=rv PROFILE=pre SDCARD=../user/build/images/wateros-rv.img
git diff --check
```

## 验证环境

- L0 宿主机：单测/check。✅
- L1 QEMU virt：GPT 分区回归。✅
- L3 真机：小镜像大卡分区识别。🔴（本次已复现 InvalidGptHeader）

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - `driver-block/block-api/api-v0/src/partition.rs`：
    `scan_gpt_at` 删除 `expected_backup_lba` 形参及 `backup_lba !=
    expected_backup_lba` 判定；保留 `backup_lba >= total_blocks` 边界检查。
- 验收结果：
  - `cargo test -p wateros-driver-block-api-v0`：4 passed。
  - `make jh7110_check` / `make rv_check`：通过。
  - QEMU virt 回归：`probed root partition /dev/vda4` → mount RW →
    rcS → login 全链路通过。
  - `make jh7110_uimage` / `jh7110_bootdir` / `make disk`：镜像重建。
  - `git diff --check`：clean。
- 真机验证（待用户重烧）：
  - 预期 GPT 分区识别成功，devfs 出现 `/dev/vda4`，fs 探测到 ext4；
  - 之后进入挂载与写路径阶段（`write_blocks` 当前 Unsupported）。
