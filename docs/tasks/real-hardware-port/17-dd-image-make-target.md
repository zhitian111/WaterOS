# 17 整盘镜像烧录目标（make dd_img_<平台>）

## 任务内容

把「烧 SD/SATA 镜像」包装成一个带确认的 Make 目标：`make dd_img_vf2
DEVICE=/dev/sdX` 与 `make dd_img_2k1000 DEVICE=/dev/sdX`。脚本先做防呆
校验（整盘而非分区、非系统盘、未挂载、容量足够），再打印目标设备信息并
要求输入 `y` 确认，确认后才执行 `dd`。

背景：真机烧录多次因盘符误选/未确认失败，需要一个不可逆操作的强制确认
入口。

## 实施方案

1. 新增 `os/scripts/real-hardware/dd_image.sh`：
   - 参数 `<image> <device>`；
   - 校验：`/dev/` 绝对路径、镜像存在、块设备、`TYPE=disk`（拒绝分区
     节点）、非系统盘、未被挂载、容量足够；
   - 展示 `lsblk` 设备信息 + 镜像/设备容量，提示输入 `y` 确认；
   - `sudo dd bs=4M conv=fsync oflag=direct status=progress` + `sync`。
2. `os/Makefile` 新增 `dd_img_vf2`（默认镜像
   `../user/build/images/wateros-rv.img`）与 `dd_img_2k1000`
   （`../user/build/images/wateros-la.img`），未传 `DEVICE` 时给出用法
   并退出；镜像路径可用 `VF2_DD_IMAGE`/`LA2K_DD_IMAGE` 覆盖。
3. 同步 `make help` 与 `os/scripts/README.md`。

## 涉及文件 / CodeGraph 查询

- `os/scripts/real-hardware/dd_image.sh`（新增）
- `os/Makefile`
- `os/scripts/README.md`

CodeGraph：本任务为宿主工具，无内核符号查询。

## 验收方式

- [ ] 未传 `DEVICE` 时 `make dd_img_vf2` 报用法并退出非零。
- [ ] 指向系统盘/分区节点/不存在设备时脚本拒绝，不执行 dd。
- [ ] 用 loop 设备端到端验证：输入 `y` 后写入，内容与源镜像一致。
- [ ] `git diff --check` 干净。

## 验收命令

```bash
cd os
make dd_img_vf2                      # 应报用法
echo y | bash scripts/real-hardware/dd_image.sh /tmp/x.img /dev/nvme0n1   # 应拒绝系统盘
dd if=/dev/zero of=/tmp/loop-test.img bs=1M count=64
sudo losetup -f --show /tmp/loop-test.img   # 记下 /dev/loopX
dd if=/dev/zero of=/tmp/small.img bs=1M count=1
echo y | bash scripts/real-hardware/dd_image.sh /tmp/small.img /dev/loopX
cmp /dev/loopX /tmp/small.img
sudo losetup -d /dev/loopX
git diff --check
```

## 验证环境

- L0 宿主机：全部可验。✅

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - 新增 `os/scripts/real-hardware/dd_image.sh`：防呆校验（`/dev/` 路径、
    镜像存在、块设备、`TYPE=disk|loop` 整盘、非系统盘、未挂载、容量足够）
    → 打印目标设备信息 → 输入 `y` 确认 → `sudo dd bs=4M conv=fsync
    oflag=direct status=progress` → `sync`。
  - `os/Makefile`：新增 `dd_img_vf2`/`dd_img_2k1000` 目标
    （`VF2_DD_IMAGE`/`LA2K_DD_IMAGE` 可覆盖，默认
    `../user/build/images/wateros-{rv,la}.img`）；help 同步。
  - `os/scripts/README.md`：脚本表补 `real-hardware/dd_image.sh` 行。
- 验收结果：
  - `make dd_img_vf2`（无 DEVICE）：报用法，rc=2。
  - 系统盘 `/dev/nvme0n1`：拒绝；分区节点 `/dev/nvme0n1p1`：拒绝（非整盘）。
  - loop 设备端到端：`printf 'y\n'` → dd 完成，读回 `cmp` 一致；
    `n` 取消不写入。
  - `make dd_img_vf2 DEVICE=/dev/zzz`：脚本设备校验生效。
  - `git diff --check`：clean。
- 未验证/风险：
  - 未对真实 SD/SATA 盘执行（真机烧录时使用，确认机制与防呆已在宿主验证）；
  - `oflag=direct` 对个别 USB 读卡器可能失败，届时去掉该参数重试即可。
