# 33 登录闭环：root 空密码

## 任务内容

让真机 `wateros login:` 能登录。已知缺口：内核 `ensure_etc_passwd` 写出的
`/etc/passwd` 中 root 密码字段为 `x`（指向 `/etc/shadow`），但未写
`/etc/shadow`，busybox login 会失败。

## 实施方案

把 `PASSWD` 中 root 条目改为空密码字段：
`root::0:0:root:/root:/bin/sh`（空字段即无密码），daemon/nobody 保持 `x`
与 `false` shell（不参与登录）。

## 涉及文件 / CodeGraph 查询

- `os/src/user_bringup_root_layout.rs`

CodeGraph：

```bash
codegraph explore "ensure_etc_passwd"
```

## 验收方式

- [ ] `make jh7110_check` / `make rv_check` 通过。
- [ ] 真机 `wateros login:` 输入 `root` 回车进入 shell。

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
- L3 真机：串口 login。🔴

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - `os/src/user_bringup_root_layout.rs`：`PASSWD` root 条目改为
    `root::0:0:root:/root:/bin/sh`（空密码字段）。
- 验收结果：
  - `make jh7110_check` / `make rv_check`：通过。
  - `make jh7110_uimage` / `jh7110_bootdir` / `make disk`：镜像重建。
  - `git diff --check`：clean。
- 真机验证（待用户重烧）：
  - `wateros login:` 输入 `root` 回车进入 shell；若输入无回显/不进 shell，
    说明串口 RX 到 tty 的输入路径尚未打通（另一小任务）。
