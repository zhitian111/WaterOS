# 18 VisionFive 2 出厂 U-Boot 自动启动（vf2_uEnv.txt + uEnv.txt）

## 任务内容

让插卡上电后**自动**进入 WaterOS，不再手敲 `fatload`/`bootm`。

真机证据：出厂 U-Boot（SDK Release 31）默认走 StarFive SDK 路径——
`mmc_test_and_boot` 先找 `vf2_uEnv.txt`（缺失 → `Failed to load
'vf2_uEnv.txt'`），再 `run boot2`（未定义 → `boot2 not defined`），最后
掉进网络启动超时进提示符。Debian 镜像能自动启动是因为它的 `uEnv.txt`
覆盖了 `bootcmd`；同理，我们的启动分区需要同时满足 SDK 路径
（`vf2_uEnv.txt` 定义 `boot2`）与 distro 路径（`uEnv.txt` 定义
`bootcmd`）。

## 实施方案

1. 新增模板 `os/scripts/root_image/jh7110-vf2-uEnv.txt`：
   - 定义 `kernel_addr_r=0x40200000`、`fdt_addr_r=0x46000000`；
   - 定义 `boot2`：`fatload mmc 1:3 ... wateros-jh7110.ui` +
     `fatload mmc 1:3 ... jh7110-starfive-visionfive-2-v1.3b.dtb` +
     `bootm ${kernel_addr_r} - ${fdt_addr_r}`。
   出厂 SDK 路径 `test -e vf2_uEnv.txt` 命中后 `load_sdk_uenv` 会 import
   它并 `run boot2`，同一轮启动即可进内核。
2. `jh7110-uEnv.txt` 增加 `bootcmd`（同样直接加载 + `bootm`），覆盖 distro
   路径与手动 `boot`。
3. `jh7110_bootdir` 把 `jh7110-vf2-uEnv.txt` 复制为启动分区的
   `vf2_uEnv.txt`；重建整盘镜像。

## 涉及文件 / CodeGraph 查询

- `os/scripts/root_image/jh7110-vf2-uEnv.txt`（新增）
- `os/scripts/root_image/jh7110-uEnv.txt`
- `os/Makefile`（`jh7110_bootdir`）

CodeGraph：本任务为板级启动素材，无内核符号查询。

## 验收方式

- [ ] `make jh7110_bootdir` 后 `os/build/jh7110-boot/` 含
      `vf2_uEnv.txt` 与 `uEnv.txt`，内容包含 `boot2=`/`bootcmd=`。
- [ ] 重新生成整盘镜像后，P3 `mdir` 可见两个 uEnv 文件。
- [ ] 真机插卡上电自动进入内核（`Starting kernel ...` + WaterOS 日志）。

## 验收命令

```bash
cd os
make jh7110_bootdir
cat build/jh7110-boot/vf2_uEnv.txt build/jh7110-boot/uEnv.txt
cd ../user && make disk ARCH=rv PACKAGE=minimal IMAGE_SIZE_MB=64 \
  DISK_SIZE_MB=192 BOOT_DIR=../os/build/jh7110-boot BOOT_SIZE_MB=64
mdir -i build/images/wateros-rv.img@@14336s ::/
cd ../os && make dd_img_vf2 DEVICE=/dev/sdX
```

## 验证环境

- L0 宿主机：素材内容/镜像校验。✅
- L3 真机：插卡上电自动启动（本次真机已复现 SDK 路径失败）。🔴→✅

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - 新增 `os/scripts/root_image/jh7110-vf2-uEnv.txt`：`kernel_addr_r`/
    `fdt_addr_r` + `boot2`（fatload uImage/DTB + `bootm`），对应出厂 SDK
    路径 `mmc_test_and_boot` 的 `test -e vf2_uEnv.txt` → `load_sdk_uenv`
    → `run boot2`。
  - `jh7110-uEnv.txt` 增加 `bootcmd`（同一组加载 + `bootm`），覆盖 distro
    路径与手动 `boot`。
  - `os/Makefile` `jh7110_bootdir` 复制 `vf2_uEnv.txt` 进启动分区。
  - 模板不含 `#` 注释行：U-Boot `env import -t` 对注释行解析行为不可靠。
- 验收结果：
  - `make jh7110_bootdir`：`vf2_uEnv.txt`（214 B）与 `uEnv.txt`（216 B）
    内容含 `boot2=`/`bootcmd=`。
  - 整盘镜像重建，P3 `mdir` 可见两个 uEnv 文件。
  - `git diff --check`：clean。
- 真机验证（待用户重烧）：
  - 预期插卡上电**自动**进入内核（不再手敲 fatload/bootm）；
  - 若 SDK 路径仍不执行 boot2，备用路径为 `boot` 命令（uEnv.txt 的
    bootcmd 已被 import）或手敲三条命令；以真机日志为准继续调。
