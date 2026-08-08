# 块缓存 16MiB A/B（已回退，2026-08-09）

## 实验内容

`BLOCK_CACHE_CAPACITY_BLOCKS` 从 16384（8MiB）试调为 32768（16MiB），观察
VirtIO/block 热点是否继续下降。

## pc-hot A/B（同 180s Final 早期窗口）

```text
基线: /tmp/pcs-rv-current-20260809b.txt
当前: /tmp/pcs-rv-blockcache-16m-20260809.txt
日志: /tmp/pc-hot-blockcache-16m-20260809.log
```

| 指标 | 8MiB | 16MiB |
|---|---:|---:|
| 总指令 | 22.94B | 23.39B |
| VirtQueue `add_notify_wait_pop` | 848M | 809M |
| `read_blocks` | 93M | 93M |

## 结论

VirtIO 下降约 5%，但总指令上升约 2%，完整耗时收益不明确。8MiB 是已验证配置，
16MiB 已回退。
