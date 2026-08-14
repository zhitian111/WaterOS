# 12 rootfs 分区整盘镜像（GPT/MBR）

## 任务内容

把 `user/` 目前产出的「无分区表 raw EXT4」升级为「带 GPT/MBR 分区表的整盘镜像」，让
SATA（2K1000）与 SD/eMMC（VisionFive 2）能挂载真正的 rootfs 分区，而不是把整块盘当
单个文件系统。

现状：`make image` 直接 `mke2fs` 出一个 `wateros-<arch>.ext4`（QEMU 用）。真机需要：

- VisionFive 2：SD/eMMC，典型为 boot 分区（FAT，放固件/DTB/内核）+ rootfs 分区（EXT4）；
- Loongson 2K1000：SATA 盘，至少一个 rootfs 分区（boot 是否单独按 PMON/uImage 而定）。

## 实施方案

1. 审计并迁移旧 `feat/real-hardware-common` 的 `scripts/root_image/root_image.py`
   （loopback 无特权构建/校验）到当前 main。
2. 在 `user/Makefile` 的 `image` 目标下增加「整盘镜像」产物（`.img`/`.qcow2`），同时
   保留 raw `.ext4` 供 QEMU 回归。
3. 支持参数化分区表：GPT（默认）与 MBR，以及 boot 分区是否启用。
4. 生成后宿主侧用 `fdisk`/`sgdisk` 校验分区，`e2fsck -fn` 校验 rootfs 分区。

## 涉及文件 / CodeGraph 查询

- `os/scripts/root_image/root_image.py`
- `user/Makefile`
- `user/README.md`（产物/参数变化同步）

CodeGraph（本任务主要是 host 工具，内核侧只涉及 devfs/分区挂载的既有符号）：

```bash
codegraph explore "register_block_device"
codegraph explore "mount"
```

## 验收方式

- [ ] 能生成带 GPT/MBR 的整盘镜像，分区表可被 `fdisk`/`sgdisk` 正确读出。
- [ ] rootfs 分区 `e2fsck -fn` 无错。
- [ ] raw EXT4 产物仍可用（QEMU 回归不破坏）。
- [ ] 参数（分区表类型/boot 分区/大小）在 README 中同步。

## 验收命令

```bash
cd user
make image ARCH=rv IMAGE_SIZE_MB=256
# 宿主侧校验分区表与文件系统
sgdisk -p build/images/wateros-rv.img    # 或 fdisk -l
e2fsck -fn <rootfs-partition 或回环挂载后>
cd ../os && make configure && make rv_check
git diff --check
```

## 验证环境

- L0 宿主机：分区表/`e2fsck` 校验。✅
- L1 QEMU virt：整盘镜像可被 QEMU 挂载冒烟。✅
- L3 真机：烧写进 SATA/SD 后挂载（后置到任务 08/10）。🔴

## 任务简报

（完成后追加，格式见目录 README。）
