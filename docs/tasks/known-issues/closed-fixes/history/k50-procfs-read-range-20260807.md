# K-50 procfs range 读取（2026-08-07）

## 问题

`FsBridge::read_range` 对 `/proc` 路径先调用 `ProcFsView::read` 整文件生成 Vec，再
切片复制。BuildStorm 反复读取 proc 文件时会造成不必要的整文件分配与复制。

## 修改

- `ProcFsView` 新增 `read_range(rel_path, offset, buf)`，默认实现基于 `read`。
- `FsBridge` 的 `/proc` 路径改走 `proc_view().read_range`。
- 内核 procfs 覆盖 `read_range`，静态 proc 文件（net/tcp、pid_max、tainted）直接
  从常量切片，不再整文件分配。

涉及文件：

- `os/components/wateros-fs/fs-procfs/procfs-api/api-v0/src/lib.rs`
- `os/components/wateros-fs/fs-procfs/procfs-impl/impl-kernel/src/lib.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/lib.rs`

## 验证

```text
make rv_check
make la_check
make kernel-rv-final
make kernel-rv-pre
```

完整 Final：

```text
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1281.26 cores=8 bytes=1681000 arch=riscv64
#### OS COMP TEST GROUP END buildstorm-glibc ####
```

这是当前本地最优记录，同时修复了 procfs 整文件读取路径。

Pre 60s smoke（P-core）：root RW 挂载成功，cyclictest、hackbench 与 LTP 用例进入
执行，无 panic 和 ext4 读块错误。

`qemu-img check`：`No errors were found on the image.`

## 可复核材料

```text
task: K-50 procfs read_range
date: 2026-08-07
kernel_commit: 768c7266 + working-tree K-50
architecture: riscv64
qemu_and_firmware: qemu-system-riscv64 virt, OpenSBI 1.7
image: os/sdcard-rv-pub.img (qcow2 overlay)
raw_log_path: /tmp/k50-full-rv-20260807.log
raw_log_sha256: e41ef6e7afec9298e5e1bc408e69d7805cb163fc53a81bb2d723100cb2182981
pre_log_path: /tmp/k50-pre-rv-20260807.log
pre_log_sha256: cac86ffb5ce1626c940530801b4f572e2d73cc07942845965e5f9d0bb5a79b0b
overlay_qemu_img_check: ok
```
