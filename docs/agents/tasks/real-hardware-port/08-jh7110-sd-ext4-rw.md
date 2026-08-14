# 08 JH7110 SD 分区 + ext4 只读→读写 + 持久化

## 任务内容

在 VisionFive 2 上打通「SD 分区挂载 → ext4 只读 → 读写 → 卸载/同步后持久化」闭环，
用任务 03 的根镜像/分区工具生成测试镜像，并用宿主机 `e2fsck -fn` 做只读一致性校验。

这是 VisionFive 2 第一块板的**真机里程碑**。

## 实施方案

1. 用 `root_image.py` 生成带 GPT/MBR 分区的 SD 测试镜像。
2. 真机从 DW MMC 枚举 SD，挂载分区，走 `impl-another-ext4` RW。
3. 依次验证：open/read → write → close/fsync/sync/unmount → 重新打开读取一致。
4. 镜像离线后用宿主机 `e2fsck -fn` 校验，确认无脏页/损坏。

## 涉及文件 / CodeGraph 查询

- `os/scripts/root_image/root_image.py`
- `os/components/wateros-fs/**`（fs bridge / page cache / ext4 适配）
- `os/components/wateros-driver/driver-block/**`

CodeGraph：

```bash
codegraph explore "mount"
codegraph explore "fsync"
codegraph explore "sync"
codegraph explore "write_block"
```

## 验收方式

- [ ] SD 分区被识别并挂载。
- [ ] ext4 只读→读写→持久化四步闭环通过，重开读取一致。
- [ ] 离线 `e2fsck -fn` 无错。

## 验收命令

```bash
cd os
# 真机烧写后执行对应 user workload / 最小 fs 读写
make configure && make rv_check
git diff --check
# 宿主机对 SD 镜像只读校验：
#   e2fsck -fn <image>
```

## 验证环境

- L0 宿主机：`e2fsck -fn`、`git diff --check`。✅
- L1 QEMU virt：ext4 RW/持久化逻辑可在 QEMU virt（VirtIO block）先回归。✅
- L3 真机：SD 真实枚举/读写/持久化。🔴（必须）

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 范围说明：任务 08 拆为 **08a（软件侧，已完成）** 与 **08b（真机里程碑，待板）**。

### 08a 完成内容（软件侧，host 可验）

- `driver-block/block-api` 升级为角色注册表（迁移自旧分支并适配 main）：
  - `BlockDeviceRole::{Disk, Partition}`、`RegisteredBlockDevice`；
  - `register_block_device` 注册整盘时自动扫描 MBR/GPT（含保护性 MBR）并注册
    有界分区设备（`PartitionBlockDevice`，任务 03 已有）；
  - 新增 `block_device_role_at` / `block_devices_snapshot` / `unregister_block_device`
    （注销整盘连带移除分区子设备）；count/first/at 按活动槽位扁平化。
- devfs：`refresh` 用角色快照生成 `/dev/vd{a..z}` 与 `/dev/vd{a..z}{1..}` 分区路径，
  替换原先硬编码指向整盘的 `/dev/vda1`/`/dev/vda2` 假别名（grep 确认无消费者依赖）。
- 恢复驱动 mmc.rs 的分区角色测试（`registration_exposes_mbr_partition_and_removes_...
  it_with_parent`，含注销连带清理）。
- 计划调整：旧分支的 `notify_device_topology_changed`（拓扑变更通知）未移植——main
  的 devfs 刷新是显式时机（driver init 后 `devfs::sync`），热插拔通知留待需要时。

### 08a 验收结果

- `cargo test -p wateros-driver-block-api-v0`：4 passed（分区解析）。
- `cargo test -p wateros-driver-impl-jh7110-visionfive2`：18 passed（含分区角色/
  注销测试）。
- 板级 feature check、`make rv_check`/`la_check`：通过；`git diff --check`：clean。

### 08b 待真机（当前环境无物理板）

- 真机烧写 SD（root_image.py 生成 MBR/GPT 镜像）→ DW MMC 枚举 → 挂载分区 →
  ext4 只读→读写→重开一致性 → 离线 `e2fsck -fn`。
- 依赖：VisionFive 2 物理板或真机串口日志；ext4 RW/持久化逻辑可在 QEMU virt
  （VirtIO 块）先回归，但 SD 真实时序必须上板。

### 08b 软件侧追加验证（2026-08-15，QEMU 实测）

- **分区根挂载打通**（本轮提交）：从 GPT 整盘镜像启动时，`fs::init` 探测与
  `mount_default_root_rw` 均回退到 `/dev/vda1`（新增 devfs `partition_block_paths()`、
  fs 探测与挂载两级回退）。
- **QEMU 实测**：以任务 12 的 `wateros-rv.img`（GPT，rootfs 分区）启动，
  日志确认 `probed root partition /dev/vda1` → `mount root RW from /dev/vda1` →
  `/sbin/init` → `[rcS] WaterOS sysinit complete` → `wateros login:`。
- 顺带修复 `root_image.py` 路径正则：允许 BusyBox 的 `[` applet（`/usr/bin/[`）。

剩余真机部分不变：SD 物理控制器枚举/读写时序必须上板验证。
