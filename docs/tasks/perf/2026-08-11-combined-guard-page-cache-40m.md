# allocator guard fastpath + 40MiB page cache 组合（2026-08-11）

## 为什么选择这里

两个独立实验均接近但未单独达到 1.5%：

- allocator guard fastpath：旧 main `880.44s` 对照两轮 `867.22/868.62s`
- 40MiB page cache：当前 main `817.27s` 对照一轮 `808.79s`

两者改动互不重叠，一个减少 allocator 固定 CSR 开销，一个提高文件缓存命中。
组合后有机会达到可验收收益，且任一失败都能独立回退。

## 方案

在 `perf/combined-guard-page-cache-40m` 分支：

1. 应用 allocator guard fastpath：中断已关闭时跳过重复 disable/restore。
2. 将 `FILE_PAGE_CACHE_CAPACITY` 从 `8192` 改为 `10240`（40MiB）。
3. 不包含任何其它未验证改动。

## 验收

- 双架构 Final check 通过。
- 完整 BuildStorm 相对当前 main 有 ≥ 1.5% 净改善。
- 无 panic/SIGSEGV/停滞。

## 实测结果（2026-08-11）

```text
combined-guard-pcache-40m-full-a1: BUILDSTORM_COMPILE ok=true elapsed_s=810.02
main-cow-full-b1:                  BUILDSTORM_COMPILE ok=true elapsed_s=817.27
```

组合候选成功跑完，无 panic/SIGSEGV，但只快约 `0.9%`，没有叠加出稳定收益。
实现已回退，仅保留本记录。
