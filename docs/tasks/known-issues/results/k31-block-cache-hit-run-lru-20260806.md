# K-31 block cache 连续命中区间批量读（2026-08-06）

## 问题

`pc-hot` 显示 `CachingBlockDevice::read_blocks` 命中时逐 512B 块调用
`touch_lru`，连续命中区间会被放大成多次 `detach + push`。8 路组相联索引落地后，
`touch_lru` 仍是 block-cache 侧 Top 热点之一。

## 修改

`read_blocks` 对连续命中区间先整段拷贝，只对区间最后一个槽刷新 LRU；未命中区间仍
合并为单次底层读。删除不再使用的 `cache_copy_out`，并新增连续命中区间回归测试。

涉及文件：

- `os/components/wateros-driver/driver-block/block-impl/impl-block-cache/src/lib.rs`

## pc-hot A/B

同一 180 秒 Final 早期阶段，fresh qcow2 overlay，8 vCPU：

| 符号 | 基线 | 当前 |
|---|---:|---:|
| 总指令 | 18.19B | 17.72B |
| `read_blocks` | 488.13M | 399.19M |
| `touch_lru` | 380.34M | 40.53M |
| `cache_put` | 165.29M | 193.32M |
| 底层 `VirtioBlkDevice::read_blocks` | 3.75M | 27 |

block-cache 主要函数合计从约 `1.03B` 降到约 `0.63B`。`cache_put` 单轮略升，可能是
同阶段噪声或 LRU 顺序变化造成，保留在下一轮低负载完整 Final 中复核。

## 验证

```text
cargo test --manifest-path os/components/wateros-driver/driver-block/block-impl/impl-block-cache/Cargo.toml
make rv_check
make la_check
make kernel-rv-final
make kernel-rv-pre
```

完整 Final：

```text
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1941.42 cores=8 bytes=1681000 arch=riscv64
#### OS COMP TEST GROUP END buildstorm-glibc ####
```

本轮 QEMU 仅约 351% CPU，宿主同时有 Firefox/桌面负载，明显慢于此前最优
`1365.70s`；完整跑通说明当前改动未破坏 CAgent/BuildStorm，但墙钟收益需低负载复测。

Pre 60 秒 smoke：root RW 挂载成功，glibc/musl cyclictest 与 hackbench 进入执行，
无 panic 和 ext4 读块错误。

`qemu-img check`：`No errors were found on the image.`

## 可复核材料

```text
task: K-31 block cache hit-run LRU batching
date: 2026-08-06
kernel_commit: 3727c056 + working-tree K-31
user_submodule_commit: 2f470f95fa6bf0401c4b1b7ef3bb8fc7a10b870b
architecture: riscv64
qemu_and_firmware: qemu-system-riscv64 virt, OpenSBI 1.7
image: os/sdcard-rv-pub.img (qcow2 overlay)
raw_log_path: /tmp/k31-full-hitrun-rv-20260806.log
raw_log_sha256: 85e1c2b9a63e72c41ea48b36b24af700ced1bd08406c91ae2656c6781a442bb2
pre_log_path: /tmp/k31-pre-hitrun-rv-20260806.log
pre_log_sha256: b779830a693a2e484db57549da4f08422658cdb460729ac792807491c3750881
pcs_current_sha256: 61e1bc7cc7ba0091cb938c48ff8a3f0e8cf903adbab1c11d14d148df195caeeb
pcs_baseline_sha256: 8c909b0fbb6e072dd5a29dd91393c549d346c624eda03ced1ea7b0ee354775fd
overlay_qemu_img_check: ok
```
