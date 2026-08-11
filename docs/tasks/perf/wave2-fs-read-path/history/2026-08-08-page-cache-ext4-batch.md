# 2026-08-08：页缓存连续池 + ext4 write-back cache + 批量释放 A/B

## 背景

在 `main` 上继续推进调度器之外的性能优化。先用页缓存连续池减少 TLSF 小对象分配，
再用 pc-hot 同窗口采样发现 `another_ext4::dealloc_block` 是最靠前的内核热点之一。

## 改动

- `wateros-vfs-impl-page-cache`：页缓存槽位不再各自持有 `Vec<u8>`，改用连续页池，
  减少 8192 次 4 KiB 堆分配。
- `another_ext4`：启用自带 write-back `block_cache`，使用 `spin::Mutex` 替代缺失的
  `axsync`，并让 adapter 在每次写后 `flush_all()`。
- `another_ext4`：新增批量 `dealloc_blocks()`，`free_inode()` 和 `extent_truncate()`
  按 block group 批量更新 bitmap/group/super，不再逐块重算整块 bitmap CRC。
- `another_ext4::CACHE_SIZE` 从 4 组扩到 64 组（16 → 256 个 4 KiB 缓存块）。

## pc-hot 同窗口结果

三组同为 RISC-V Final 早期 200s 窗口，QEMU 8 vCPU / 8 GiB / `-snapshot`，绑定
P-core。第一组只含页缓存连续池；第二组启用 ext4 write-back cache；第三组加入批量
释放。

| 轮次 | 总指令 | `another_ext4::dealloc_block` | VirtQueue `add_notify_wait_pop` | `CachingBlockDevice::read_blocks` |
|---|---:|---:|---:|---:|
| 页缓存连续池 | 26.82B | 3.40B | 1.66B | 0.66B |
| + ext4 write-back cache | 24.72B | 3.34B | 1.33B | 0.12B |
| + 批量释放 | 25.51B | 不存在（`dealloc_blocks` 65.6M） | 1.14B | 0.18B |

## 材料

```text
pcs_1: /tmp/pcs-rv-pagecache-contig-20260808.txt
  sha256: 37f0a9ebed70b52eece1e339c3f9c42b41753ca29d29c157aeee5f81ffc1a486
pcs_2: /tmp/pcs-rv-ext4-cache-20260808.txt
  sha256: 51992ad379f81822f408a866ead2a3645d507fd6de6eb78f9716f2c88b7866d9
raw_2: /tmp/ext4-cache-pc-hot.log
  sha256: 247b8f4a212b1183da2c2d30f4f3ab02a8ddcecde09722c2177f765c270f084f
pcs_3: /tmp/pcs-rv-ext4-batch-20260808.txt
  sha256: 1e59c295b64bc4083596e5ec9d78d4cb5a8eeb0bfcc65ed8d51e0c852c87e0b8
raw_3: /tmp/ext4-batch-pc-hot.log
  sha256: 7c01d4f2c804f4fa7a5e6fb163e002c8c970022276f50ef113f439130655215a
pre_smoke: /tmp/ext4-batch-pre-smoke.log
  sha256: 27f53e146dfb15fcffb7150e4bff79c16fa9ac26e554cc344becd3d6c9eeb4d0
cache64_pre_smoke: /tmp/ext4-cache64-pre-smoke.log
  sha256: 239fbeed557bea3b5f914491f3db4f0ff7171e6b15f58577c7079eab95ecabe5
```

三轮均推进到 `BUILDSTORM_TOOLCHAIN ok` / `BUILDSTORM_MINIBUILD ok` 并进入
`pre-build tg-xtask`；pre smoke 进入 LTP 执行，无 panic/fatal trap。

## 结论与决策

- 页缓存连续池保留：减少每槽 4 KiB 堆分配和 TLSF 锁竞争。
- ext4 write-back cache 保留：VirtIO/block cache 热点明显下降；flush 日志从
  `info` 降到 `trace`，避免串口放大。
- 批量 `dealloc_blocks` 保留：`dealloc_block` 3.4B → 消失，释放路径改为按 block
  group 一次读取/写回 bitmap、group、super，不再逐块重算整块 CRC。
- `CACHE_SIZE` 扩容后尚未重采，下一次 Final/early 窗口再记录收益。
- 还需要在最终门禁补：完整 BuildStorm、iozone/LTP FS 回归、运行后 `e2fsck -fn`，
  以及掉电/崩溃一致性复测。
