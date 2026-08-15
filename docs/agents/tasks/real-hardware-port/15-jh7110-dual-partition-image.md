# 15 JH7110 双分区整盘镜像（FAT boot P3 + ext4 rootfs P4）

## 任务内容

把 `user/` 的 `make disk` 整盘镜像升级为 VisionFive 2 可直接烧录的布局：
GPT 四分区（P1/P2 固件占位 2M/4M，P3 FAT 启动分区，P4 ext4 rootfs），
启动分区内容来自任务 14 的 `jh7110_bootdir` 素材目录。

出厂 U-Boot 默认 `bootpart=3` / `rootpart=4`，distro 路径直接从 P3
`sysboot /extlinux/extlinux.conf`（官方 Debian 镜像同路径）。分区编号与官方
镜像对齐后，插卡执行 `boot` 即可自动启动 WaterOS，无需手敲命令。

## 实施方案

1. `user/tools/root_image.py`：
   - 新增 `--boot-dir` / `--boot-size-mib`（默认 64）；
   - `make_partition_table_vf2`：P1 2MiB、P2 4MiB 占位，P3 FAT boot，
     P4 ext4 rootfs（GPT 与 MBR 两套 sfdisk 规格）；
   - `build_boot_partition`：`mkfs.vfat` + `mmd`/`mcopy`（mtools）把
     `--boot-dir` 内容写进 P3，再整块写入镜像分区偏移；
   - `verify_boot_partition`：抽验 P3 每个文件与源内容一致；
   - `verify_image` 支持四分区布局：P3 文件校验 + P4 原有
     `e2fsck -fn`/superblock/路径校验。
2. `user/tools/image.py` `create_disk_image` 透传 boot 参数；
   `user/tools/userland.py` 的 `image` 命令新增 `--boot-dir`/
   `--boot-size-mb`；`user/Makefile` 新增 `BOOT_DIR`/`BOOT_SIZE_MB`。
3. 同步 `user/README.md` 与 `os/scripts/README.md`。

## 涉及文件 / CodeGraph 查询

- `user/tools/root_image.py`
- `user/tools/image.py`、`user/tools/userland.py`、`user/Makefile`
- `user/README.md`、`os/scripts/README.md`

CodeGraph（确认内核侧分区探测路径，镜像布局与 `/dev/vdX4` 挂载对应）：

```bash
codegraph explore "mount_default_root_rw"
codegraph explore "partition_block_paths"
codegraph explore "register_block_device"
```

## 验收方式

- [ ] `make disk ARCH=rv PACKAGE=minimal BOOT_DIR=<jh7110-boot>
      BOOT_SIZE_MB=64 DISK_SIZE_MB=192` 产出 GPT 四分区镜像；
      `fdisk -l` 分区号/大小与 P3 boot、P4 rootfs 一致。
- [ ] `mdir`/`mcopy` 抽验 P3 文件与 `BOOT_DIR` 内容一致。
- [ ] P4 `e2fsck -fn` 通过。
- [ ] QEMU virt 整盘启动：日志出现 rootfs 探测到 `/dev/vda4` →
      mount root RW → `/sbin/init` → getty 提示。
- [ ] `git diff --check` 干净。

## 验收命令

```bash
cd user
make disk ARCH=rv PACKAGE=minimal IMAGE_SIZE_MB=64 DISK_SIZE_MB=192 \
  BOOT_DIR=../os/build/jh7110-boot BOOT_SIZE_MB=64
fdisk -l build/images/wateros-rv.img
mdir -i build/images/wateros-rv.img@@14336s ::/
cd ../os && make configure && make rv_check
git diff --check
```

> `mdir -i <img>@@<offset>` 的偏移格式为 mtools 的字节偏移，实际验收以
> `sgdisk`/`fdisk` 读到的 P3 起始扇区换算为准。

## 验证环境

- L0 宿主机：分区表、P3 FAT 内容、P4 `e2fsck` 全可验。✅
- L1 QEMU virt：整盘镜像挂载 rootfs 冒烟（`/dev/vda4`）。✅
- L3 真机：烧录后 U-Boot 自动 `boot` → 内核串口打印（后置任务 08b）。

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - `user/tools/root_image.py`：新增 `--boot-dir`/`--boot-size-mib`；
    `make_partition_table_vf2` 生成 P1 2M、P2 4M、P3 FAT boot、P4 ext4
    rootfs 四分区（GPT：P3 用 Microsoft basic data GUID
    `EBD0A0A2-...`，避免 sfdisk `S` 简写误标成 swap；MBR：P3 `0c`）；
    `build_boot_partition` 用 `mkfs.vfat -n WATEROS` + `mmd`/`mcopy` 填充
    P3 后整块写入镜像；`verify_boot_partition` 逐文件 `mcopy` 抽验内容；
    `verify_image` 支持四分区布局。
  - `user/tools/image.py`/`userland.py`：`create_disk_image` 透传
    `--boot-dir`/`--boot-size-mb`；`user/Makefile` 新增 `BOOT_DIR`/
    `BOOT_SIZE_MB` 并接入 `disk` 目标。
  - `user/README.md`、`os/scripts/README.md`：同步新参数与 VF2 布局说明。
- 验收结果：
  - `make disk ARCH=rv PACKAGE=minimal IMAGE_SIZE_MB=64 DISK_SIZE_MB=192
    BOOT_DIR=../os/build/jh7110-boot BOOT_SIZE_MB=64` 产出
    `user/build/images/wateros-rv.img`（192 MiB）。
  - `fdisk -l`：P1 2M / P2 4M / P3 64M Microsoft basic data /
    P4 121M Linux filesystem。
  - `mdir -i ...@@14336s ::/`：boot.scr、extlinux/extlinux.conf、
    jh7110-starfive-visionfive-2-v1.3b.dtb、uEnv.txt、wateros-jh7110.ui
    全部存在；`mcopy` 抽验 boot.scr 内容一致。
  - P4 抽离后 `e2fsck -fn`：clean。
  - QEMU virt 整盘冒烟（`make run ARCH=rv PROFILE=pre
    SDCARD=../user/build/images/wateros-rv.img`）：
    `probed root partition /dev/vda4` → `mount root RW from /dev/vda4` →
    `/sbin/init` → `[rcS] WaterOS sysinit complete` → `wateros login:`。
  - `make rv_check` 通过；`git diff --check`：clean。
- 未验证/风险：
  - 真机烧录与出厂 U-Boot `boot` 自动启动（后置任务 08b）；QEMU 只验证了
    rootfs 挂载路径，U-Boot 对 P3 的实际 fatload/sysboot 行为仍需上板。
  - P1/P2 占位分区无文件系统，仅用于对齐出厂 U-Boot 的
    `bootpart=3`/`rootpart=4` 编号；若后续板级固件更新改变编号，需同步。
