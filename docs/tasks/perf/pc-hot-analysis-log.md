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
