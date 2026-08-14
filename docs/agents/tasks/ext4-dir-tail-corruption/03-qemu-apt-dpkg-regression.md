# 任务 03：QEMU 端到端回归（内核级目录边界 + 可选 apt/dpkg）

## 状态

fs 模式已完成并通过；apt 模式被 main 分支缺失的 syscall 兼容阻断（见下文证据）。

## 目标

在 RISC-V QEMU 中验证任务 01 的修复：

- 默认模式 `REGRESS_MODE=fs`：guest 直接用 shell 构造“目录块填满到 12 字节
  tail 边界 + 3 字符子目录”的写路径，宿主对 overlay 执行 e2fsck，验证无乱码
  目录项。该模式不依赖 apt/网络。
- 可选模式 `REGRESS_MODE=apt`：真实执行 `apt-get install neovim-runtime`，
  验证原始报错场景。

## 测试镜像

- 使用解压出来的工作副本：
  `/home/zhitian/project/WaterOS_refactor/os/sdcard-rv-pub.img`（约 14G）。
- 原始镜像保持在 `~/Downloads/sdcard-rv-pub.img.gz`，不会被修改。
- 首次使用前对工作副本执行过一次 `e2fsck -fy`，清除镜像自带 journal 的
  陈旧事务与空闲计数偏差；该偏差在解压后即存在，与本次任务无关。

## 涉及文件

- `os/scripts/regress_ext4_dir_tail.sh`（本次新增）

脚本固定使用 qcow2 overlay（backing 为只读工作副本），guest 写盘不落回基准
镜像；QEMU 启动通过 `scripts/run/rv_qemu_run_snapshot.sh`，满足 snapshot 约束。

## 实施方案

1. 注入 guest 脚本 `/root/regress_dir_tail.sh` 到工作副本。
2. 构建 `MODE=run SCRIPT=/root/regress_dir_tail.sh` 的 operator-run 内核。
3. 用 qcow2 overlay 启动 QEMU，guest 执行脚本后内核自动关机。
4. 将 overlay 转 raw，先 `e2fsck -fy`（回放 journal/修复计数），再
   `e2fsck -fn` 只读校验。
5. 日志只 grep 关键标记，不输出全量 QEMU 日志。

## CodeGraph 查询命令

```sh
codegraph explore "sys_lseek sys_ioctl unlockpt"
codegraph node "os/src/user_operator.rs"
codegraph files
```

索引不可用时回退：

```sh
rg -n "operator-run|WATEROS_OPERATOR_SCRIPT|rv_qemu_run_snapshot" os/Makefile os/scripts
```

## 验收命令

```sh
cd /tmp/WaterOS_ext4_dir_tail_fix/os
REGRESS_MODE=fs bash scripts/regress_ext4_dir_tail.sh
```

验收标准（fs 模式）：

- guest 输出 `REG DIR TAIL FSCK PASS`；
- `e2fsck -fy` 与 `e2fsck -fn` 均通过，Pass 2 无 `illegal characters`、
  `fails checksum` 等目录结构错误；
- 基准镜像未被 guest 写穿（使用 overlay）。

## apt/dpkg 模式的已知阻断

`REGRESS_MODE=apt` 在 main（本分支基座）上复现了原始安装路径，但 apt 在
解包阶段提前中止，证据如下（`os/tem/regress-dir-tail.qemu.log`）：

```text
E: Unlocking the slave of master fd 11 failed! - unlockpt (22: Invalid argument)
dpkg: error processing archive .../neovim-runtime_0.10.4-8_all.deb (--unpack):
 cannot skip padding for file './usr/share/applications/nvim.desktop':
 failed to seek (Operation not supported)
dpkg-deb: error: paste subprocess was killed by signal (Broken pipe)
```

另外 guest 中 `grep -P` 从管道读 stdin 时返回
`grep: (standard input): Operation not supported`。

这三个问题都是 syscall 层兼容缺口（PTY `unlockpt`/`TIOCSPTLCK`、普通文件
seek、管道读），与 ext4 目录块 tail 修复无关，也不是本任务引入的。队友此前
声称的 `unlockpt/ptsname` 与 `lseek ESPIPE` 修复不在 main HEAD
（`59f50c44`）中，需要先合入对应分支的 syscall 修复后，apt 模式才能作为
本任务的验收项。

## 完成后简报

写 `history/03-qemu-apt-dpkg-regression-brief.md`，记录两种模式的结果与
apt 阻断证据。
