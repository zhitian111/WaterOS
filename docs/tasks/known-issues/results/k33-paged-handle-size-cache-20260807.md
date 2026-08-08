# K-33 PagedFileHandle 读路径避免逐次 ext4 metadata 查询（2026-08-07）

## 问题

`pc-hot` 显示 `PagedFileHandle::current_size` 是内核侧 Top-10 热点，约 331M
指令。它每次 read/seek/write 都会锁 `DetachedState`、克隆 cache key，并对 stable
node 调用 `metadata()`，再锁一次页缓存。BuildStorm 大量小读放大了该路径。

## 修改

`current_size` 不再逐次调用 `stable.metadata()`，改用句柄打开/截断时记录的
`on_disk_size` 作为页缓存 `logical_size` 的 fallback。内核内所有页缓存写/截断路径
都会同步更新 `GlobalFilePageCache` 的逻辑大小，因此读路径无需再每次锁 ext4。

涉及文件：

- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/paged_handle.rs`

## pc-hot A/B

同一 180 秒 Final 早期阶段，fresh qcow2 overlay，8 vCPU：

| 符号 | K-33 基线 | K-33 当前 |
|---|---:|---:|
| 总指令 | 17.25B | 17.03B |
| `PagedFileHandle::current_size` | 331.45M | 4.12M |
| `metadata_node` | 9.65M | 0.67M |
| `logical_size` | 23.51M | 24.25M |

`current_size` 热路径下降约 99%，ext4 metadata 锁基本退出读路径。

## 验证

```text
cargo test --manifest-path os/components/wateros-vfs/vfs-impl/impl-page-cache/Cargo.toml
make rv_check
make la_check
make kernel-rv-final
make kernel-rv-pre
```

完整 Final：

```text
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1873.87 cores=8 bytes=1681000 arch=riscv64
#### OS COMP TEST GROUP END buildstorm-glibc ####
```

完整墙钟从 K-31/K-32 的约 1941-1957s 降到 `1873.87s`，约改善 4%；仍高于 K-30 曾记录
的 `1365.70s`，且未达到 700-800s 目标。后续瓶颈仍需转向 TLSF、VirtIO 和页缓存
`purge_closed_file`。

Pre 60 秒 smoke：root RW 挂载成功，glibc/musl cyclictest 与 hackbench 进入执行，
无 panic 和 ext4 读块错误。

`qemu-img check`：`No errors were found on the image.`

## 可复核材料

```text
task: K-33 PagedFileHandle current_size fast path
date: 2026-08-07
kernel_commit: 8fdb047a + working-tree K-33
user_submodule_commit: 2f470f95fa6bf0401c4b1b7ef3bb8fc7a10b870b
architecture: riscv64
qemu_and_firmware: qemu-system-riscv64 virt, OpenSBI 1.7
image: os/sdcard-rv-pub.img (qcow2 overlay)
raw_log_path: /tmp/k33-full-rv-20260807.log
raw_log_sha256: a0512045c8cf7e4f7db6eb6ba4383a594a982b050136628ffd5d87a9e87d3937
pre_log_path: /tmp/k33-pre-rv-20260807.log
pre_log_sha256: 39a1474d8de79661af4b01a321a125877795b5298c53c4bd1541d8e3598686a9
pcs_baseline_sha256: 3a4d2f178eb192691cf07ed1835821a1604334e781e1ab324612667ffd4edaf3
pcs_current_sha256: 06cb720b358708dc9a4c9aad4b967d5da5ca72242a2d57ea0459ef0bb4c6bbb0
overlay_qemu_img_check: ok
```
