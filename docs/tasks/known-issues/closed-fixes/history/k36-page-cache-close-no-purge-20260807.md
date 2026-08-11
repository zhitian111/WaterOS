# K-36 最后 close 不立即 purge 页缓存（2026-08-07）

## 问题

`pc-hot` 显示 `purge_closed_file` 约 350M 指令，是页缓存侧最大热点之一。BuildStorm
会大量 open/close 文件，而旧实现每次最后一个句柄关闭都会扫描并删除该文件全部缓存页，
既增加 close 开销，也让后续 reopen 重新读盘。

## 修改

- `release_open_ref` 在最后一个句柄关闭时只移除 `open_refs`/`files` 元数据，不再
  调用 `purge_closed_file`。
- 新增 `forget_closed_file`，只清理路径元数据，缓存页继续由页缓存 LRU 保留。
- unlink/rename 等需要失效路径缓存的路径仍调用 `purge_closed_file` 强制清页。

涉及文件：

- `os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs`

## pc-hot A/B

同一 180 秒 Final 早期阶段，基线 K-35：

| 符号 | 基线 | 当前 |
|---|---:|---:|
| 总指令 | 17.10B | 16.49B |
| `purge_closed_file` | 350.08M | 0.03M |
| `file_key` | 3.53M | 3.53M |

`purge_closed_file` 基本退出早期 Final 热点，总指令下降约 3.6%。

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
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1881.13 cores=8 bytes=1681000 arch=riscv64
#### OS COMP TEST GROUP END buildstorm-glibc ####
```

完整墙钟为 `1881.13s`，与 K-33/K-35 的 `1873-1896s` 处于噪声范围；该改动显著降低
close 热路径指令，但整轮仍由其它用户态/TLSF/VirtIO 热点主导。

Pre 60 秒 smoke：root RW 挂载成功，cyclictest、hackbench 与 LTP 早期用例进入执行，
无 panic 和 ext4 读块错误。

`qemu-img check`：`No errors were found on the image.`

## 可复核材料

```text
task: K-36 close no immediate page-cache purge
date: 2026-08-07
kernel_commit: f3bf2006 + working-tree K-36
user_submodule_commit: 2f470f95fa6bf0401c4b1b7ef3bb8fc7a10b870b
architecture: riscv64
qemu_and_firmware: qemu-system-riscv64 virt, OpenSBI 1.7
image: os/sdcard-rv-pub.img (qcow2 overlay)
raw_log_path: /tmp/k36-full-rv-20260807.log
raw_log_sha256: ce43404c64caf4850610b235688f948bc515bbf65ca982e55badad468db0f131
pre_log_path: /tmp/k36-pre-rv-20260807.log
pre_log_sha256: 3ae3678bad5280818f7d51a6b7094a26b13be3876591020c07e23a59f49a6d52
pcs_current_sha256: d9913a13bc5a2cb4e694d3a37a924c3b1c8a5ccb1225b65b85f5dbf7b2ca380f
overlay_qemu_img_check: ok
```
