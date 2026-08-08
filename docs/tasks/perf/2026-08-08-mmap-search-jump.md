# 2026-08-08：mmap 空闲区间搜索跳转

## 背景

pc-hot 中 `find_free_mmap_base_considering_vmas` 仍在 Top 20。原实现遇到已映射页、
lazy VMA 或共享匿名 VMA 冲突时只前进一页，大量时间花在重复的页表 walk 和区间检查上。

## 改动

RISC-V Sv39 与 LoongArch64 同步修改：

- 新增 `lazy_vma_overlap_end()`：按有序 lazy VMA 快速定位第一个冲突 VMA 的结束地址。
- 新增 `shared_anon_vma_overlap_end()`：返回冲突共享匿名 VMA 的最大结束地址。
- 新增栈和内核保留区间跳转；页表扫描遇到已映射页时跳到下一页。
- 每次冲突至少前进一整页，并保留 `MAX_SEARCH_PAGES` 上限。

## pc-hot 同窗口结果

同为 RISC-V Final 早期 200s 窗口，QEMU 8 vCPU / 8 GiB / `-snapshot`，绑定 P-core。

| 指标 | block cache 连续池 | + mmap 搜索跳转 |
|---|---:|---:|
| `find_free_mmap_base_considering_vmas` | 180.5M | 137.5M |

目标符号下降约 24%。总指令受运行波动影响，不用于本轮验收。

## 材料

```text
pcs: /tmp/pcs-rv-mmap-jump-20260808.txt
  sha256: 7c40dc254fa6bc2bb18bf36cf2766a61222ab74bd71ec0ebcdca671ddf3ef872
raw_log: /tmp/mmap-jump-pc-hot.log
  sha256: ff3ddaaece0b45c0acd949b6588211142da5b5246890bfefe8965db5ff6d8508
pre_smoke: /tmp/mmap-jump-pre-smoke.log
  sha256: 8165c2c47a9a5b8faf4d47fc37eb7f16ea18689d4aaed763d9c0b83a3d5a0f11
```

## 验证

- `make check ARCH=rv PROFILE=final` 通过
- `make check ARCH=la PROFILE=final` 通过
- RISC-V pre smoke 进入 LTP，无 panic/fatal

## 后续

- 完整 BuildStorm、mmap/mprotect/mremap 定向回归和最终门禁仍需补充。
- 下一个主要候选仍为 TLSF 全局锁与 `memcpy/memcmp` 来源拆分。
