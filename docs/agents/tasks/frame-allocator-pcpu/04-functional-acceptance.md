# Task 04：最终功能 / bug 验收

## 任务目标

在最终分支上完成双架构完整功能回归，证明 frame batch 与低风险优化没有功能回归。

## 验收机制

1. 每架构从 `~/Downloads/*.img.gz` 重新解压新镜像，覆写 buildstorm 脚本；
2. 使用 QEMU 9.2.1 和线上等价参数，连续 3 轮；
3. 所有 QEMU 运行加 `-snapshot`；
4. 每轮必须满足 TOOLCHAIN/MINIBUILD/BUILDSTORM OK，且无 panic/OOM/SIGSEGV/
   fault 异常。

## 静态检查

```bash
cd /tmp/wateros-frame-allocator-pcpu/os
HEAP_ALLOCATOR_FEATURE=heap-slab make rv_check
HEAP_ALLOCATOR_FEATURE=heap-slab make la_check
git diff --check
```

## 完成后

新增 `history/04-brief.md`，记录 6 轮结果和日志 SHA。

