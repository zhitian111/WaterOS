# Task 02 简报：slab 补页接入 per-CPU frame batch

## 完成情况

`HeapFrameSourceAdapter` 的 `alloc_frame` / `dealloc_frame` 改为调用 batch 接口，
使 slab 小对象页补充优先走 per-CPU batch。

## 改动文件

- `os/src/main.rs`

## 验收

```text
HEAP_ALLOCATOR_FEATURE=heap-slab make rv_check   PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make la_check   PASS
git diff --check                                  PASS
```

## 未验证项

- 完整 BuildStorm 功能与性能已补测首轮；多轮中位数尚未完成。

## 首轮 BuildStorm 结果

```text
RV 8 核 16G:
  /tmp/wateros-frame-pcpu-rv-smp8-16g-r1.log
  BUILDSTORM_RESULT ... elapsed_s=561.80 run=OK

LA 12 核 16G:
  /tmp/wateros-frame-pcpu-la-smp12-16g-r1.log
  BUILDSTORM_RESULT ... elapsed_s=545.87 run=OK
```

对照 slab 基线首轮/中位数：

```text
RV slab: 560.09 / 565.32
LA slab: 539.30 / 539.30
```

首轮看 frame batch 在 RV 上接近 slab，在 LA 上略有回退；尚不足以证明明显收益，
需要更多轮次或转向串行编译阶段优化。
