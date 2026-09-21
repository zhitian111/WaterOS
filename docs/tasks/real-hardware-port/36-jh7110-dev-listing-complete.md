# 36 补全用户空间 `/dev` 设备列举

## 任务内容

`ls /dev` 目前只有 `ptmx pts shm tty`：devfs 已注册的块设备
（`/dev/vda*`）、字符设备（`/dev/ttyS0`、`/dev/console`、`/dev/null`、
`/dev/rtc0`）与内置特殊节点（`/dev/zero`、`/dev/urandom`、`/dev/random`）
未合并进目录视图。

## 实施方案

1. `impl-fs-bridge` 的 `merge_special_dev_children` 改为合并完整节点集合：
   `fs::devfs::active_impl::list_nodes()`（块/字符）+ pty 特殊路径 +
   `/dev/zero`、`/dev/urandom`、`/dev/random`。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/lib.rs`

CodeGraph：

```bash
codegraph explore "merge_special_dev_children"
codegraph explore "list_dev_nodes"
```

## 验收方式

- [ ] `make rv_check` / `make jh7110_check` 通过。
- [ ] 真机 `ls /dev` 出现 `vda vda1..4 ttyS0 console null rtc0 zero ...`。

## 验收命令

```bash
cd os
make rv_check && make jh7110_check
make jh7110_uimage && make jh7110_bootdir
cd ../user && make disk ARCH=rv PACKAGE=minimal IMAGE_SIZE_MB=64 \
  DISK_SIZE_MB=192 BOOT_DIR=../os/build/jh7110-boot BOOT_SIZE_MB=64
git diff --check
```

## 验证环境

- L0 宿主机：check。✅
- L3 真机：`ls /dev`。🔴

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - `impl-fs-bridge` `merge_special_dev_children`：改为合并
    `fs::devfs::active_impl::list_nodes()`（块/字符）+ pty 特殊路径 +
    `/dev/zero`、`/dev/urandom`、`/dev/random`（按路径去重）。
- 验收结果：
  - `make rv_check` / `make jh7110_check`：通过。
  - `make jh7110_uimage` / `jh7110_bootdir` / `make disk`：镜像重建。
  - `git diff --check`：clean。
- 真机验证（待用户重烧）：
  - 预期 `ls /dev` 出现 `vda vda1..4 ttyS0 console null rtc0 zero urandom ...`；
  - 块设备仅“可见”，`open`/`read` 句柄是后续任务。
