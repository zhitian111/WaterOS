# 2026-08-08：block cache 连续块池

## 背景

上一轮已确认 `another_ext4::dealloc_block` 通过批量释放解决。下一轮继续看内存与
VirtIO/block 热点：`CachingBlockDevice` 仍有 16384 个独立 `Vec<u8>` 槽位，初始化时
会产生大量 512 字节堆分配，也会让热路径 LRU 命中时跨分散内存复制。

## 改动

- `wateros-driver-block-impl-block-cache`：`Slot` 不再持有 `Vec<u8>`。
- `CachingBlockDevice` 增加一块连续 `Vec<u8>` 块池，通过 `slot_data()` /
  `slot_data_mut()` 访问。
- 16384 个 512 字节槽位合并为一次约 8 MiB 的连续分配。

## pc-hot 同窗口结果

同为 RISC-V Final 早期 200s 窗口，QEMU 8 vCPU / 8 GiB / `-snapshot`，绑定 P-core。

| 指标 | 批量释放后 | + block cache 连续池 |
|---|---:|---:|
| 总指令 | 25.51B | 25.13B |
| `memcpy` | 6.98B | 6.93B |
| `memset` | 1.12B | 0.94B |
| VirtQueue `add_notify_wait_pop` | 1.14B | 0.76B |
| `CachingBlockDevice::read_blocks` | 176M | 91M |
| TLSF `allocate` | 1.06B | 1.08B |

结论：VirtIO/block 路径和 `memset` 有明显下降；TLSF `allocate` 基本持平，说明
剩余分配热点不在 block cache 初始化/热路径本身。

## 材料

```text
pcs: /tmp/pcs-rv-blockcache-contig-20260808.txt
  sha256: 70fa7c1e669c302d0b508de78bca2f9c8f0a52e833ad0e211d103acbd6b642f2
raw_log: /tmp/blockcache-contig-pc-hot.log
  sha256: b354183051bd9ece40c5ea047a68ffe302a39feae3c57effec745c1edefa32e5
pre_smoke: /tmp/blockcache-contig-pre-smoke.log
  sha256: 006da8246db635beab73eb91f170c0bf9aa5359bce4b75721eee2061622bbd6e
```

## 验证

- `make check ARCH=rv PROFILE=final` 通过
- `make check ARCH=la PROFILE=final` 通过
- RISC-V pre smoke 进入 LTP，无 panic/fatal
- 单元测试受宿主编译 RISC-V `sbi-rt` 汇编依赖阻塞，未在宿主直接运行

## 后续

- 完整 BuildStorm、iozone/LTP FS 回归、`e2fsck -fn`、掉电一致性仍需在最终门禁验证。
- 下一候选热点：TLSF 全局锁和 `memcpy/memcmp` 来源拆分。
