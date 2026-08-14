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

（完成后追加，格式见目录 README。）
