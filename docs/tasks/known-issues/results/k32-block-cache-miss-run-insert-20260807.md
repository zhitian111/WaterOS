# K-32 block cache 连续 miss 区间直接插入（2026-08-07）

## 问题

K-31 后 `pc-hot` 显示 `CachingBlockDevice::read_blocks` 的连续 miss 区间仍逐块调用
`cache_put`，而 miss 区间在扫描阶段已经确认这些 LBA 不在索引中。逐块插入会重复执行
一次 `LbaIndex::get`。

## 修改

新增 `cache_put_new`，用于调用方已确认 LBA 不在索引中的路径；`read_blocks` 的 miss
区间改用它直接分配槽位、插入索引并刷新 LRU。`write_blocks` 等未知状态的路径仍走
原 `cache_put`。

涉及文件：

- `os/components/wateros-driver/driver-block/block-impl/impl-block-cache/src/lib.rs`

## pc-hot A/B

同一 180 秒 Final 早期阶段，fresh qcow2 overlay，8 vCPU：

| 指标 | K-32 基线 | K-32 当前 |
|---|---:|---:|
| 总指令 | 18.14B | 17.32B |
| `read_blocks` | 440.78M | 535.31M |
| `cache_put` | 184.89M | 被内联 |
| `cache_put_new` | - | 被内联 |
| `touch_lru` | 43.28M | 39.46M |
| block-cache 合计 | 668.95M | 574.77M |

`cache_put_new` 被 Rust 编译器内联进 `read_blocks`，所以当前 `read_blocks` 单独计数
上升；按 block-cache 函数合计，当前比基线少约 14%。

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
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1957.45 cores=8 bytes=1681000 arch=riscv64
#### OS COMP TEST GROUP END buildstorm-glibc ####
```

本轮完整 Final 可跑通，但墙钟仍约 `1957s`，没有把完整 BuildStorm 拉到 700-800s；
说明当前剩余开销主要由用户态编译、TLSF、VirtIO 和 MM 路径组成，block-cache 指令
下降尚未转化为整轮时间收益。

Pre 60 秒 smoke：root RW 挂载成功，glibc/musl cyclictest 与 hackbench 进入执行，
无 panic 和 ext4 读块错误。

`qemu-img check`：`No errors were found on the image.`

## 可复核材料

```text
task: K-32 block cache miss-run direct insert
date: 2026-08-07
kernel_commit: bbffe256 + working-tree K-32
user_submodule_commit: 2f470f95fa6bf0401c4b1b7ef3bb8fc7a10b870b
architecture: riscv64
qemu_and_firmware: qemu-system-riscv64 virt, OpenSBI 1.7
image: os/sdcard-rv-pub.img (qcow2 overlay)
raw_log_path: /tmp/k32-full-rv-20260807.log
raw_log_sha256: ba1d1a240c3527793925275d4ff9160fd17d8f137599c89c3df834fefba0a0b4
pre_log_path: /tmp/k32-pre-rv-20260807.log
pre_log_sha256: c24ee862c13892a1adedfbba32a93d42c311500879ea56dbae0ca77a4303ba07
pcs_baseline_sha256: 4919388539c55c3464eb3cac17564bcc0bfce38c7d11d7771719d033fc2111c8
pcs_current_sha256: 436c117f5d7289821f887e3e6983f3d75d6b57619e852a49ccd9d12a3f966336
overlay_qemu_img_check: ok
```
