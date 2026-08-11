# 绝对路径规范化快速路径 A/B（已回退，2026-08-08）

## 实验内容

`normalize_absolute_path` 增加字节级快速路径：对已经满足“以 `/` 开头、无重复 `/`、
无 `.`/`..`、无尾部 `/`”的路径直接 `String::from` 返回，避免原有 split/push/truncate
流程。

## pc-hot A/B（同 180s Final 早期窗口）

```text
基线: /tmp/pcs-rv-pagecache-range-20260808.txt
当前: /tmp/pcs-rv-path-fast-20260808.txt
日志: /tmp/pc-hot-path-fast-20260808.log
```

| 指标 | 基线 | 当前 |
|---|---:|---:|
| 总指令 | 22.01B | 23.68B |
| `normalize_absolute_path` | 137.8M | 175.7M |
| `memcpy` | 6.07B | 6.85B |
| `memcmp` | 2.86B | 2.96B |

## 结论

快速路径没有降低 `normalize_absolute_path` 的指令数，总指令也明显高于基线。
可能是额外扫描抵消了原有流程收益，或该窗口内进度差异放大了噪声；当前没有净收益，
已回退，不进入完整 Final 验证。
