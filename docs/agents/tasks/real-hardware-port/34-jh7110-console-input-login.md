# 34 控制台输入 + login 闭环

## 任务内容

让真机 `wateros login:` 能输入并登录，消除 getty respawn 循环。

根因（两处）：

1. getty 按 inittab 打开 `/dev/ttyS0`，但 JH7110 的机器初始化先注册了
   rtc/null 占位（0/1），控制台 UART 落到索引 2（`/dev/ttyS2`），
   `ttyS0` 不存在 → open 失败 → busybox init 反复 respawn getty。
2. 控制台 TTY 默认 `Closed`，且 UART RX → `tty::feed_input` 的轮询任务
   只在 operator 模式启动；userspace-init 模式没有启动，getty 读不到输入。

## 实施方案

1. `impl-jh7110-visionfive2/lib.rs`：先注册控制台 UART，再注册 rtc/null
   占位，使 UART 成为索引 0（`/dev/ttyS0` + `/dev/console`），与共享
   inittab 对齐。
2. `os/src/user_operator.rs`：`start_console_input_task` 改 `pub(crate)`。
3. `os/src/user_bringup_init.rs`：进入 userspace-init 前
   `tty::configure(Interactive)` 并启动控制台输入任务。
4. `user/rootfs/base/etc/inittab`：仅保留 `ttyS0` getty（`ttyS1` 无设备，
   其 respawn 刷屏会干扰 ttyS0 的 shell 提示）。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-driver/driver-impl/impl-jh7110-visionfive2/src/lib.rs`
- `os/src/user_operator.rs`
- `os/src/user_bringup_init.rs`
- `user/rootfs/base/etc/inittab`

CodeGraph：

```bash
codegraph explore "poll_console_input_once"
codegraph explore "try_start_init"
codegraph explore "register_builtin_character_devices"
```

## 验收方式

- [ ] `make jh7110_check` / `make rv_check` 通过。
- [ ] QEMU virt 回归：init/getty 仍可达。
- [ ] 真机 login 只输出一次，`root` 回车进入 shell，输入有回显。

## 验收命令

```bash
cd os
make jh7110_check && make rv_check
make jh7110_uimage && make jh7110_bootdir
cd ../user && make disk ARCH=rv PACKAGE=minimal IMAGE_SIZE_MB=64 \
  DISK_SIZE_MB=192 BOOT_DIR=../os/build/jh7110-boot BOOT_SIZE_MB=64
git diff --check
```

## 验证环境

- L0 宿主机：check。✅
- L1 QEMU virt：回归。✅
- L3 真机：串口 login。🔴

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - `impl-jh7110-visionfive2/lib.rs`：控制台 UART 先于 rtc/null 注册，
    落到索引 0（`/dev/ttyS0` + `/dev/console`）。
  - `os/src/user_operator.rs`：`start_console_input_task` 改 `pub(crate)`。
  - `os/src/user_bringup_init.rs`：userspace-init 前
    `tty::configure(Interactive)` 并启动控制台输入任务。
  - `user/rootfs/base/etc/inittab`：删除 `ttyS1` getty。
- 验收结果：
  - `make jh7110_check` / `make rv_check`：通过。
  - QEMU virt 回归：`root` 回车进入 `/root # `，`echo`/`uname -a`/
    `cat /etc/passwd` 正常执行。
  - `make jh7110_uimage` / `jh7110_bootdir` / `make disk`：镜像重建。
  - `git diff --check`：clean。
- 真机验证（待用户重烧）：
  - 预期 login 只出现一次，`root` 回车进入 `/root # `，输入有回显。
