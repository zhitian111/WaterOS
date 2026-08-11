# 页缓存稳定 node key（2026-08-09）

## 优化思路

页缓存旧 key 是 `(mount_gen, path, page_index)`，稳定文件节点的 `cache_key`
虽然已压缩为 `@node:<mount>:<node>` 字符串，但 BTree 每次比较仍会走字符串
`memcmp`。BuildStorm 热路径中 `memcmp` 一直是内核侧最大可控热点之一。

本次把 `FileCacheKey` 扩展为：

- `stable: Option<(mount_id, node_id)>`
- `path: Arc<str>` 继续保留给底层 `PageCacheIo` 使用

`Ord/PartialEq/Hash` 对稳定节点只比较 `(mount_gen, mount_id, node_id)`，路径字符串
不参与 BTree 比较；没有稳定 node 的路径键仍按原路径比较。

同时补齐与稳定 key 配套的路径：

- `read_key` / `write_key` / `logical_size_key`
- `acquire_open_ref_key` / `release_open_ref_key` / `truncate_key`
- `flush_key`，并让 `flush_all` 按真实 key 遍历

## pc-hot A/B（180s Final 早期窗口，合并后同基线）

```text
基线: /tmp/pcs-rv-merged-baseline-20260809.txt
当前: /tmp/pcs-rv-merged-current-20260809.txt
```

| 指标 | 基线 | 当前 |
|---|---:|---:|
| 总指令 | 22.709B | 22.729B |
| `memcmp` | 2.855B | 676.9M |
| tuple BTree `search_tree` | 172.3M | 退出 Top-50 |
| `FileCacheKey::cmp` | - | 123.4M |
| `memcpy` | 6.279B | 7.456B |

`memcmp` 下降约 76%，页缓存索引查找从字符串比较变成数字比较；总指令基本持平，
`memcpy`/TLSF 有一定成本迁移。完整耗时方向为正，因此保留。

## 完整 Final

合并后基线 RISC-V：

```text
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1076.82 cores=8 arch=riscv64
```

带本优化：

```text
RISC-V:  elapsed_s=1037.27 cores=8 arch=riscv64
LoongArch: elapsed_s=1002.57 cores=8 arch=loongarch64
```

日志：

- `/tmp/final-after-merge-baseline-rv-20260809.log`
- `/tmp/final-after-latest-scheduler-rv-20260809.log`
- `/tmp/final-after-latest-scheduler-la-rerun-20260809.log`

中间一次 LoongArch 完整轮出现过 `SIGSEGV`，随后原样重跑通过；已把该轮记入
`/tmp/final-after-latest-scheduler-la-20260809.log`，不作为有效成绩。

另跑 120s RISC-V pre smoke，日志
`/tmp/pre-smoke-pagecache-stable-20260809.log`，无 panic。
