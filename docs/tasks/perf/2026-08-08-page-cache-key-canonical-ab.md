# page-cache 规范化 key / Arc 指针比较 A/B（已回退，2026-08-08）

## 实验内容

在 page-cache `FileCacheKey` 上做两个改动：

1. `read/write` 从 `files` 表取回规范化 `FileCacheKey`，后续 BTree 查找复用同一个
   `Arc<str>`。
2. `FileCacheKey` 的 `Ord/PartialEq` 在 `mount_gen` 相同且 `Arc::ptr_eq` 成立时
   直接短路，避免重复字符串比较。

目标是降低页缓存 BTree 查找中的 `memcmp`。

## pc-hot A/B（同 180s Final 早期窗口）

基线为已提交的 page-cache range 版本，当前为实验版本：

```text
基线: /tmp/pcs-rv-pagecache-range-20260808.txt
当前: /tmp/pcs-rv-canonical-key-20260808.txt
日志: /tmp/pc-hot-canonical-key-20260808.log
```

| 指标 | 基线 | 当前 |
|---|---:|---:|
| 总指令 | 22.01B | 23.29B |
| `memcpy` | 6.07B | 7.01B |
| `memcmp` | 2.86B | 2.16B |
| TLSF `allocate` | 869M | 1007M |
| TLSF `deallocate` | 613M | 704M |
| `FileCacheKey::cmp` | - | 141M |

## 结论

`memcmp` 下降约 24%，但实验引入了规范化 key 的额外 Arc clone、`FileCacheKey::cmp`
调用和 `get_key_value` 路径，总指令数上升约 5.8%，`memcpy` 与 TLSF 也变差。
该改动不是净收益，已回退，不进入完整 Final 验证。

## 纯 `FileCacheKey::cmp` 指针短路复测

随后只保留 `FileCacheKey` 的 Arc 指针短路 `Ord/PartialEq`，不回传规范化 key：

```text
基线: /tmp/pcs-rv-pagecache-range-20260808.txt
当前: /tmp/pcs-rv-arc-ord-only-20260808.txt
日志: /tmp/pc-hot-arc-ord-only-20260808.log
```

| 指标 | 基线 | 当前 |
|---|---:|---:|
| 总指令 | 22.01B | 23.65B |
| `memcmp` | 2.86B | 2.77B |
| `FileCacheKey::cmp` | - | 186M |

纯指针短路仍不能抵消新增比较函数开销，总指令上升约 7.4%，同样回退。
