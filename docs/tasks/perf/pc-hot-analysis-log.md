# pc-hot 分析记录

每次 pc-hot 采样完成后，把采样数据、对比结论和后续决策记录在这里。原始 `pcs.txt`
保留在 `/tmp`，本文件只保存摘要与可复核路径。

## 2026-08-07 K-32 基线（K-31 commit `bbffe256`）

运行：

```text
run_id: pc-hot-k32-baseline-20260807
date: 2026-08-07 00:31 CST
kernel_commit: bbffe256 (K-31)
workload: Final 早期 180s，cagent + buildstorm toolchain
qemu: qemu-system-riscv64, 8 vCPU, 8 GiB, fresh qcow2 overlay
pcs_path: /tmp/pcs-rv-k32-baseline-20260807.txt
pcs_sha256: 4919388539c55c3464eb3cac17564bcc0bfce38c7d11d7771719d033fc2111c8
raw_log: /tmp/pc-hot-k32-baseline-20260807.log
```

Top-25 摘要：

| 排名 | 计数 | 符号 |
|---|---:|---|
| 1 | 3.56B | `compiler_builtins::memcpy` |
| 2 | 3.07B | `memcmp` |
| 3 | 1.83B | `memset` |
| 4 | 1.28B | `??` |
| 5 | 0.86B | VirtQueue `add_notify_wait_pop` |
| 6 | 0.54B | TLSF `allocate` |
| 7 | 0.49B | `handle_user_page_fault` |
| 8 | 0.44B | block cache `read_blocks` |
| 9 | 0.38B | TLSF `deallocate` |
| 10 | 0.38B | `PagedFileHandle::current_size` |
| 11 | 0.31B | page cache `purge_closed_file` |
| 16 | 0.18B | block cache `cache_put` |

分析：

- block cache `read_blocks` + `cache_put` 仍在内核 Top-20。
- 连续 miss 区间已经先扫描索引，再逐块调用 `cache_put`，会重复一次 `LbaIndex::get`。
- 决策：实现 `cache_put_new`，让 miss 区间直接插入，跳过第二次索引查找。

## 2026-08-07 K-32 当前（工作树 `cache_put_new`）

运行：

```text
run_id: pc-hot-k32-current-20260807
date: 2026-08-07 00:35 CST
kernel_commit: bbffe256 + K-32 working tree
workload: Final 早期 180s，cagent + buildstorm toolchain
qemu: qemu-system-riscv64, 8 vCPU, 8 GiB, fresh qcow2 overlay
pcs_path: /tmp/pcs-rv-k32-current-20260807.txt
pcs_sha256: 436c117f5d7289821f887e3e6983f3d75d6b57619e852a49ccd9d12a3f966336
raw_log: /tmp/pc-hot-k32-current-20260807.log
```

与基线同窗口对比（block-cache 相关指令）：

| 指标 | K-32 基线 | K-32 当前 |
|---|---:|---:|
| 总指令 | 18.14B | 17.32B |
| `read_blocks` | 440.78M | 535.31M |
| `cache_put` | 184.89M | 被内联 |
| `cache_put_new` | - | 被内联 |
| `touch_lru` | 43.28M | 39.46M |
| block-cache 合计 | 668.95M | 574.77M |

分析：

- `cache_put_new` 被 Rust 编译器内联进 `read_blocks`，所以当前 `read_blocks` 单独计数
  上升；按 block-cache 函数合计，当前比基线少约 14%。
- 当前仍保留该优化，进入完整 Final 和 Pre smoke 验收。

后续结果：

- 完整 Final 通过：`elapsed_s=1957.45`。
- 完整墙钟未改善，仍约 1957s；说明当前剩余瓶颈在 block-cache 之外。
- 决策：保留 K-32，因为 block-cache 热路径下降约 14%，且完整 Final/Pre 均通过；
  后续优化应转向 TLSF、VirtIO 和 MM/页缓存路径。

## 2026-08-07 K-30 基线复测（未完成）

为了判断 K-31/K-32 是否拖慢整轮，使用 K-30 commit `3727c056` 重新跑完整 Final。
`axbuild` 已输出 `done (1822.66s)`，但 7 分钟后仍未打印 `BUILDSTORM_COMPILE`，
命中 K-01 已记录的 `cargo xtask` 返回竞态，随后终止本轮。

```text
run_id: k30-full-rerun-20260807
date: 2026-08-07 01:17 CST
kernel_commit: 3727c056
result: inconclusive (known post-build return race)
raw_log: /tmp/k30-full-rv-20260807.log
```

分析：

- 本轮不能证明 K-31/K-32 造成完整轮回退。
- K-32 当前内核在同宿主可完整跑通，且已提交。

## 2026-08-07 K-33 基线（K-32 commit `8fdb047a`）

运行：

```text
run_id: pc-hot-k33-baseline-20260807
date: 2026-08-07 02:00 CST
kernel_commit: 8fdb047a (K-32)
workload: Final 早期 180s，cagent + buildstorm toolchain
qemu: qemu-system-riscv64, 8 vCPU, 8 GiB, fresh qcow2 overlay
pcs_path: /tmp/pcs-rv-k33-baseline-20260807.txt
pcs_sha256: 3a4d2f178eb192691cf07ed1835821a1604334e781e1ab324612667ffd4edaf3
raw_log: /tmp/pc-hot-k33-baseline-20260807.log
```

关键热点：

- `PagedFileHandle::current_size`：331.45M
- `metadata_node`：9.65M
- page cache `purge_closed_file`：305.41M

分析：

- 每次 read/seek/write 都调 `current_size`，其中再逐次锁 ext4 `metadata_node`。
- 决策：读路径改为依赖页缓存逻辑大小 + 句柄打开/截断时记录的磁盘大小，不再逐次
  查询 ext4 metadata。

## 2026-08-07 K-33 当前（工作树 current_size 优化）

```text
run_id: pc-hot-k33-current-20260807
date: 2026-08-07 02:03 CST
kernel_commit: 8fdb047a + K-33 working tree
workload: Final 早期 180s，cagent + buildstorm toolchain
pcs_path: /tmp/pcs-rv-k33-current-20260807.txt
pcs_sha256: 06cb720b358708dc9a4c9aad4b967d5da5ca72242a2d57ea0459ef0bb4c6bbb0
raw_log: /tmp/pc-hot-k33-current-20260807.log
```

关键对比：

| 符号 | 基线 | 当前 |
|---|---:|---:|
| 总指令 | 17.25B | 17.03B |
| `PagedFileHandle::current_size` | 331.45M | 4.12M |
| `metadata_node` | 9.65M | 0.67M |
| `logical_size` | 23.51M | 24.25M |

分析：

- `current_size` 热路径下降约 99%，ext4 metadata 锁基本退出读路径。
- 决策：进入完整 Final 和 Pre smoke 验收；若语义回归则回退。

## 2026-08-07 K-34 页缓存反向索引（已回退）

运行：

```text
run_id: pc-hot-k34-baseline-20260807
date: 2026-08-07 02:43 CST
kernel_commit: 5587cb76 (K-33)
pcs_path: /tmp/pcs-rv-k34-baseline-20260807.txt
pcs_sha256: b243f730817ff91e5a5f1d5fa8546cf033942e8e06f5b24b32b9d21d91ee9de3
raw_log: /tmp/pc-hot-k34-baseline-20260807.log

run_id: pc-hot-k34-current-20260807
kernel_commit: 5587cb76 + K-34 working tree
pcs_path: /tmp/pcs-rv-k34-current-20260807.txt
pcs_sha256: a33d511e0503dba35026aaa9751402cb1f4bb04ff7db33365606151c6963640a
raw_log: /tmp/pc-hot-k34-current-20260807.log
```

关键对比：

| 符号 | 基线 | 当前 |
|---|---:|---:|
| 总指令 | 17.27B | 17.06B |
| `purge_closed_file` | 350.89M | 351.09M |

分析：

- 反向索引去掉了全表扫描（原 `from_iter` 约 11.9M），但新增 `BTreeMap<FileCacheKey,
  BTreeSet<u64>>` 的插入/删除维护成本基本抵消，`purge_closed_file` 无净收益。
- 随后改用 `BTreeMap::range` 只遍历目标文件页键，避免全表扫描且不引入反向索引，
  但 `purge_closed_file` 仍约 `351.16M`，没有净收益。
- 决策：回退 K-34 两个版本，不进入完整 Final。

```text
run_id: pc-hot-k34b-current-20260807
pcs_path: /tmp/pcs-rv-k34b-current-20260807.txt
pcs_sha256: 1677b13aaaf5581fd0ce9d59ad7f8f8f55daa5c5a6931db5e0429cf84834b0e5
raw_log: /tmp/pc-hot-k34b-current-20260807.log
```
