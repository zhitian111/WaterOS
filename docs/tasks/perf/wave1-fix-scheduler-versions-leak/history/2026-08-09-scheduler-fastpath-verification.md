# 调度/进程生命周期快速路径验证（2026-08-09）

## 背景

`79552d61` 时的 RISC-V Final 为 `elapsed_s=1035.07`。随后加入 subreaper 与
PDEATHSIG 生命周期语义后，完整轮耗时上升；本文件记录为减少该回归所做的快速路径：

- FIFO/RR 队列 `highest_priority()` 在 `task_count == 0` 时直接返回 `None`，
  避免每次调度 tick 扫描 99 个优先级桶。
- `ProcessRegistry` 维护 `subreaper_count`；系统没有任何 subreaper 时，
  托孤直接回到 init，不再逐层搜索祖先。

## 结果

| 配置 | RISC-V `elapsed_s` |
|---|---:|
| `79552d61` 基线 | 1035.07 |
| subreaper 语义后 | 1094.26 |
| 当前 PDEATHSIG + subreaper | 1148.24 |
| 加 subreaper 快速路径 | 1136.35 |
| 再加 FIFO/RR 空队列快速路径 | 1116.56 |

验证：

```text
make check ARCH=rv PROFILE=final 通过
make check ARCH=la PROFILE=final 通过
RISC-V Final BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1116.56
```

波动尚未完全消除，后续若需要继续追到 1050s，应先在同一提交上连续多轮取中位数，
再逐项回退 subreaper/PDEATHSIG 生命周期改动做 A/B。
