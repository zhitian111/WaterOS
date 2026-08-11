# page-cache 索引清理改用 BTree range（2026-08-08）

## 背景

最新 pc-hot 180s Final 采样中，页缓存索引清理仍出现：

- `Keys::next`：246.07M
- `search_tree`：172.65M
- `purge_closed_file` 的 `from_iter`：103.38M

这些开销来自 `purge_closed_file/finish_rename/truncate` 使用 `.keys().filter()`
全表扫描后收集 Vec。

## 改动

`os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs`：

- `purge_closed_file` 使用 `BTreeMap::range((key,0)..=(key,u64::MAX))`。
- `finish_rename` 分别按 old/new key 使用 range。
- `truncate` 按 `(key, first_past_eof)..=(key,u64::MAX)` 使用 range。

## pc-hot A/B（同 180s Final 早期窗口）

| 符号 | 基线 | 当前 |
|---|---:|---:|
| `Keys::next` | 246.07M | 未进入 Top-50 |
| page-cache `from_iter` | 103.38M | 未进入 Top-50 |
| `search_tree` | 172.65M | 174.24M |

原始采样：

```text
基线: /tmp/pcs-current-20260808.txt
当前: /tmp/pcs-rv-pagecache-range-20260808.txt
```

## 结论

改动消除全表键扫描和一次临时 Vec 收集；`search_tree` 因 range 查找略有上升但低于
原全表遍历成本。可保留进入后续完整 Final 复验。
