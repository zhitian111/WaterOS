# Task 04 简报：跨核 free、partial/empty slab 管理

## 完成情况

完成跨核 free 基础实现：

- 每 CPU 维护 `remote_head`（spinlock 保护）intrusive remote-free 队列；
- 非 owner CPU free 时把对象压入 owner CPU 队列，owner 在下一次 alloc 前 drain；
- 本地 dealloc 只在 owner CPU 执行，避免同一页进入多个 CPU 的 partial 链；
- partial/empty 页继续复用，不触发 frame allocator 回收递归。

## 改动文件

- `runtime-heap-allocator/src/slab/cpu_slab.rs`
- `runtime-heap-allocator/src/slab/slab_cache.rs`
- `runtime-heap-allocator/src/slab/mod.rs`

## 验收命令与结果

```text
HEAP_ALLOCATOR_FEATURE=heap-slab make rv_check   PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make la_check   PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-rv-final  PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-la-final  PASS
git diff --check                                 PASS
```

运行时：

- LA `-smp 12` 完整 buildstorm：`status=OK rc=0 elapsed_s=700.10 run=OK`，
  日志 `/tmp/wateros-slab-la-task04-smp12.log`，
  SHA-256 `8e82c44645c200b6a17e66e02c4719bffc6d384037daed2ded2b2927620fce23`；
- RV `-smp 8` 三连失败，均为 guest `SIGSEGV`；同镜像 baseline `-smp 8` 对照
  也失败（`/tmp/wateros-slab-rv-baseline-smp8-compare.log`），判定为 **RV 既有间歇
  问题，不是 slab 引入**；
- RV 单核 slab 完整 buildstorm 已在 Task 03 通过，说明 slab 路径本身可用。

## 未验证项

- RV `-smp 8` 最终功能回归需在修复既有 SIGSEGV 后完成；
- remote-free 队列当前用 spinlock，性能路径后续可升级 lock-free。
