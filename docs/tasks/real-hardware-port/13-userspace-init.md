# 13 userspace-init 化（/sbin/init → rcS/inittab → getty）

## 任务内容

把内核启动从「直接 exec operator shell」改为「执行 `/sbin/init`，由 init 挂载伪文件系统、
跑 `/etc/init.d/rcS`、再按 `/etc/inittab` 起 getty/shell」。这是让 rootfs 成为「正经根文件
系统」的关键一步。

现状证据：`user/rootfs/base/etc/init.d/rcS` 注释明确写 "WaterOS currently launches the
operator shell directly. This file is kept as the future userspace-init entry"；内核
`user_bringup_*` 现在直接起 shell。

## 实施方案

1. busybox 提供 `init` applet（确认当前 busybox 配置已开，否则启用并重新构建）。
2. `rootfs/base` 增加 `/etc/inittab`：串口 `getty`（`ttyS0`/`ttyS1`，按板选择）、
   `::sysinit:/etc/init.d/rcS` 等。
3. 内核 `user_bringup` 增加「init 模式」：rootfs 存在 `/sbin/init` 时优先 exec 它，
   否则回退到现有 operator shell（保留 `operator-shell` feature 行为）。
4. 确认 devfs 把 `/dev`（tty、console、block、ptmx/pts）挂好，getty 能打开 tty。

## 涉及文件 / CodeGraph 查询

- `os/src/user_bringup_bus.rs`、`user_bringup_common.rs`、`user_bringup_root_layout.rs`
- `user/rootfs/base/etc/inittab`（新增）、`user/packages/busybox/**`
- `os/components/wateros-fs/fs-devfs/**`

CodeGraph：

```bash
codegraph explore "user_bringup"
codegraph explore "execve"
codegraph explore "open"
codegraph explore "devfs"
```

## 验收方式

- [ ] QEMU 启动后走 `/sbin/init → rcS → getty`，能从串口登录并进入 shell。
- [ ] `operator-shell` 回退路径仍可用（无 init 时不 panic）。
- [ ] `/proc`、`/sys`、`/dev` 被 init 正确挂载/填充。

## 验收命令

```bash
cd user
make image ARCH=rv PACKAGE=minimal
cd ../os
make shell ARCH=rv PROFILE=pre SDCARD=../user/build/images/wateros-rv.ext4
make configure && make rv_check
git diff --check
```

## 验证环境

- L0 宿主机：`cargo check`。✅
- L1 QEMU virt：init/getty 全链路可完整验证。✅
- L3 真机：串口 getty 在板级 UART 上验证（后置到 08/09 联调）。🔴

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - 新增 `user/rootfs/base/etc/inittab`（busybox init 格式）：
    `::sysinit:/bin/sh /etc/init.d/rcS`（显式解释器——WaterOS execve 尚未支持
    shebang，直跑脚本会 Exec format error）、`ttyS0`/`ttyS1` 串口 getty respawn。
  - `rcS` 改为 userspace-init 的 sysinit 脚本（更新注释，末尾输出
    `[rcS] WaterOS sysinit complete` 到 /dev/console 便于验证）。
  - 新增 `os/src/user_bringup_init.rs`：`try_start_init()` 探测 rootfs `/sbin/init`，
    存在则派生内核任务执行它（`run_one_elf_argv_exit`，等待；init 退出则停机）；
    不存在/探测失败返回 `false` 回退。
  - `user_bringup_busybox::run_stage_busybox` 优先走 init 模式，回退保留
    operator/LTP 队列（比赛镜像无 `/sbin/init`，不受影响）。
- 验收结果（QEMU virt，RISC-V，minimal 镜像实测）：
  - 启动日志：`/sbin/init present; entering userspace-init mode` →
    `launching /sbin/init` → **`[rcS] WaterOS sysinit complete`** →
    **`wateros login:`** getty 提示出现。
  - `make rv_check`、`make la_check`：通过；`git diff --check`：clean。
  - busybox 配置确认：`CONFIG_INIT=y`、`CONFIG_GETTY=y`、`CONFIG_LOGIN=y`、
    `CONFIG_FEATURE_USE_INITTAB=y`、`CONFIG_INSTALL_APPLET_SYMLINKS=y`
    （`/sbin/init`、`/sbin/getty` 由 busybox install 自动安装，镜像内已核实）。
- 未验证/风险：
  - 登录闭环未完成：getty 提示出现但未验证输入登录（passwd 用 `x`，需
    `/etc/shadow` 或改为空密码策略；串口输入到 tty 的路径待进一步验证）。
  - 回退路径（无 `/sbin/init` 的镜像）仅编译验证，未跑运行时回退（需比赛镜像）。
  - `/dev` 挂载与 getty tty 打开依赖既有 devfs/字符设备路径，已在 QEMU 验证
    getty 能打开 ttyS0；`/proc` 由内核挂载，`/sys` 尚无 sysfs（未实现）。
  - 真机串口 getty 后置 08/09 联调。
