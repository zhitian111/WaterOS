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

（完成后追加，格式见目录 README。）
