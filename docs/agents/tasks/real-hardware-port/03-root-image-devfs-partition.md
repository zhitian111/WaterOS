# 03 根镜像与 devfs 分区/稳定 slot

## 任务内容

从 `feat/real-hardware-common` 迁移两块板都要用的「根镜像构建 + 烧写」工具和
「devfs 动态拓扑/稳定 slot/GPT/MBR 分区」能力。

- `scripts/root_image/root_image.py`（loopback 无特权构建/验证镜像）；
- devfs 动态拓扑与稳定 slot，GPT/MBR 分区解析，支撑后续 SD（任务 08）与 SATA（任务 10）
  的持久化挂载。

## 实施方案

1. 迁移并审计 `root_image.py`：输入镜像大小、分区表（GPT/MBR）、根卷内容。
2. 迁移 devfs 分区/稳定 slot 改动，按当前 main 的 devfs 现状重新接合（不整文件覆盖）。
3. 补镜像构建/分区解析的 host 级验证（生成小镜像 → `e2fsck -fn` 只读校验）。

## 涉及文件 / CodeGraph 查询

- `os/scripts/root_image/root_image.py`
- `os/components/wateros-fs/fs-devfs/**`
- 相关 partition/GPT/MBR 解析模块

CodeGraph：

```bash
codegraph explore "register_block_device"
codegraph explore "partition"
codegraph explore "mount"
```

## 验收方式

- [ ] `root_image.py` 能在 host 无特权构建出合法镜像，`e2fsck -fn` 无错。
- [ ] GPT/MBR 分区解析有单测或脚本验证。
- [ ] devfs 稳定 slot 语义在 QEMU 冒烟中不回归。

## 验收命令

```bash
cd os
python3 scripts/root_image/root_image.py --help
make configure && make rv_check && make la_check
git diff --check
# 生成测试镜像后用宿主机 e2fsck -fn 做只读一致性检查
```

## 验证环境

- L0 宿主机：工具与镜像校验。✅
- L1 QEMU virt：挂载/分区路径冒烟。✅
- L3 真机：不涉及（工具级）。❌

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - 新增 `os/scripts/root_image/root_image.py`（自旧分支迁移，525 行）：loopback
    无特权构建 MBR/GPT 整盘镜像（`sfdisk` + `mkfs.ext4 -E offset=`），`verify`
    子命令做分区/`e2fsck -fn`/`dumpe2fs`/`debugfs` 校验。
  - 新增 `driver-block/block-api/api-v0/src/partition.rs`（自旧分支迁移并适配）：
    `scan_mbr` / `scan_gpt` / `PartitionBlockDevice`；适配 main 的 `BlockDevice`
    trait（补 `flush` 到分区视图与测试 MemoryDisk）。host 单测 4 个通过
    （主分区扫描、分区读写翻译与越界拒绝、坏表拒绝、GPT 条目与损坏回退）。
  - `block-api/lib.rs` 注册 `pub mod partition;`。
  - `os/scripts/README.md` 同步 root_image 入口。
- 计划调整（基于当前 main 证据）：
  - devfs 的 `/dev/vdaN` 分区路径与块设备注册表 `BlockDeviceRole`（Disk/Partition）
    **顺延**：旧分支该能力绑定了一次 fs-api 大重构（handles/traits/types 合并），
    直接搬会与 main 的 VFS 冲突。本任务落地「分区解析 + 分区视图设备」这一可测试
    核心；分区设备的注册与 devfs 路径在任务 08/10（SD/SATA 挂载）落地时按 main
    架构接合。main 的 devfs 未被改动，无回归面。
- 验收结果：
  - `cargo test -p wateros-driver-block-api-v0`：4 passed（host）。
  - `root_image.py build/verify`：MBR 与 GPT 各构建 32 MiB 镜像并 verify 通过
    （内部含 `e2fsck -fn`）；`fdisk -l` 确认分区表。
  - `make rv_check`、`make la_check`：通过（仅既有 warnings）。
  - `git diff --check`：clean。
- 未验证/风险：
  - 分区视图设备的真实控制器读写（SD/SATA）未验证，待任务 08/10。
  - root_image.py 依赖宿主 `sfdisk`/`mkfs.ext4`/`e2fsck`/`debugfs`/`dumpe2fs`，
    已在当前环境确认可用。
