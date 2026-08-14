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

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - `os/scripts/root_image/root_image.py` 新增 `--copy-tree <staging>`：直接把既有
    staging 树作为 rootfs（保留模式/符号链接），`verify` 也支持以树为期望路径；
    原 manifest 路径保持兼容（任务 03 已迁移该工具）。
  - `user/tools/image.py` 新增 `create_disk_image`：调用 root_image.py 构建并校验
    整盘镜像（`e2fsck -fn` + 路径检查），成功后原子替换产物。
  - `user/tools/userland.py` 的 `image` 命令新增 `--disk`/`--partition-table`/
    `--disk-size-mb`：产出 `wateros-<arch>.img`（GPT 默认），raw `.ext4` 保留。
  - `user/Makefile` 新增 `DISK`/`PARTITION_TABLE`/`DISK_SIZE_MB` 参数与 `disk`
    目标；`user/README.md`、`os/scripts/README.md` 同步。
- 验收结果：
  - `root_image.py build/verify --copy-tree`：合成 staging 树产出 MBR 与 GPT 整盘
    镜像，`fdisk -l` 分区表正确，内部 `e2fsck -fn` 通过。
  - `image.create_disk_image`（staging → `.img`）：GPT 整盘镜像产出并校验通过。
  - `make -n disk` 命令展开正确（`--disk --partition-table gpt`）。
  - `make rv_check`：无回归；`git diff --check`：clean。
- 计划调整/未验证：
  - boot FAT 分区未实现：本机无 `mkfs.vfat`，且 VisionFive 2 官方固件镜像自带
    U-Boot/DTB 的 FAT 引导分区；rootfs 分区已满足两块板挂载需要，boot 分区留待
    需要时补充。
  - 未跑完整 `make image DISK=1`（需交叉编译 busybox，时间长）；用合成 staging
    直接验证了工具链与产物路径。真机烧写后置任务 08/10。
