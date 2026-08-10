# page cache 40MiB 容量 A/B（2026-08-11）

## 为什么选择这里

当前 `FILE_PAGE_CACHE_CAPACITY = 8192`（32MiB）。历史记录中 16MiB → 32MiB 有约
5% 收益，48MiB 曾出现停滞。BuildStorm 会访问大量源码和产物文件，32MiB 可能仍
不足够容纳活跃工作集。

40MiB 是 32MiB 与 48MiB 的中间值，预计能提高 cache hit，又不会像 48MiB 那样显著
增加页帧池/替换压力。

## 选择的方案

将 `FILE_PAGE_CACHE_CAPACITY` 从 `8192` 改为 `10240`（40MiB）。

## 为什么这么做

1. 这是纯容量配置改动，不改变替换算法、索引、dirty 协议或 I/O 语义。
2. 若 40MiB 有收益，再考虑按阶段细化；若退化，回退成本为零。

## 接下来的工作

1. 在 `perf/page-cache-capacity-40m` 分支修改容量。
2. 双架构 Final check 与 180 秒 smoke。
3. RISC-V 完整 BuildStorm A/B；相对当前 main 有 ≥ 1.5% 净改善才合并。
4. 完成后补 pc-hot/wait-hot 并归档。

## 验收标准

- 双架构 Final check 通过。
- 完整 BuildStorm 无 panic/SIGSEGV/停滞。
- 相对当前 main 有可复现收益。

## 实测结果（2026-08-11）

```text
page-cache-40m-full-a1: BUILDSTORM_COMPILE ok=true elapsed_s=808.79
main-cow-full-b1:       BUILDSTORM_COMPILE ok=true elapsed_s=817.27
```

完整 BuildStorm 成功，无 panic/SIGSEGV，约快 `8.48s`（1.0%），未达到 1.5% 合并
门槛。实现已回退，仅保留本记录；后续可把它作为低风险叠加项与其他稳定改动合并。
