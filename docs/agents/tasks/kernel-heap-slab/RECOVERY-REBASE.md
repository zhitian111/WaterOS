# Slab 任务恢复与后续 Rebase 说明

本文件是 `perf/kernel-heap-slab` 分支暂停在 VMA 重构前的恢复入口。保留当前工作树，
不要丢弃；VMA 分支完成后，按这里恢复并 rebase slab 工作。

## 分支与工作树

- 分支：`perf/kernel-heap-slab`
- 工作树：`/tmp/wateros-kernel-heap-slab`
- 当前 HEAD：`890ef8d1`
- 原始基线：`59f50c44`

## 已完成

- Task 00–07 已提交；
- LA 12 核性能中位：`552.44s`，baseline `562.56s`；
- RV 8 核当前候选修复通过一轮：`568.54s`，但关闭了 `elf-lazy-map`；
- RV 单核完整通过。

## 关键提交

```text
890ef8d1 [heap-slab] 08 记录 RV 候选修复与待验证状态
a113f3ab [heap-slab] 08 lazy VMA 二分查找增加线性回退防漏查
9f3ab46f [heap-slab] 08 RV SIGSEGV 候选修复：Sv39 默认关闭 elf-lazy-map
3f8f181c [heap-slab] 07 优化 remote-free CAS 与 realloc 同 class 快路径
74900584 [heap-slab] 07 增加 slab 分配统计与调参开关文档
46533dab [heap-slab] 06 实现连续帧大对象路径并默认关闭防宿主机 OOM
c1cc0b12 [heap-slab] 05 完成大对象回退与 realloc 正确性验证
abc6a719 [heap-slab] 04 实现跨核 remote-free 队列与 owner CPU drain
ac626872 [heap-slab] 03 接入 boot TLSF + frame-backed slab 切换并修复帧回收递归
36921ada [heap-slab] 02 实现未接线的 per-CPU slab 核心
873bdc61 [heap-slab] 01 抽象 GlobalAlloc 后端为 KernelAllocator + HeapBackend
db9418fa [heap-slab] 00 完成两架构 buildstorm 基线采集
```

## 已知阻塞

- RV 8 核最终验收需要无其他 QEMU 窗口；
- 候选修复关闭 Sv39 `elf-lazy-map`，RV 性能约慢 1%，不能作为长期方案；
- 真正的根因应在 VMA 分支解决，之后重新打开 `elf-lazy-map`。

## VMA 分支完成后如何恢复

1. 检查 VMA 分支是否已经把 VMA 有序性/统一路径修好；

   当前 VMA 分支已完成，最新 HEAD 为：

   ```text
   fffad37f97d6e0c72efb992756ab2f1222990139
   ```

   关键结果：

   - RV 单核 / RV 8 核 BuildStorm：`OK`
   - LA 单核 / LA 12 核 BuildStorm：`OK`
   - LA 12 核最终 36G `elapsed_s=513.19`
   - 静态检查：`make rv_check`、`make la_check`、
     `make kernel-rv-final`、`make kernel-la-final` 全部通过

2. 把 `perf/kernel-heap-slab` rebase 到 VMA 分支：

   ```bash
   cd /tmp/wateros-kernel-heap-slab
   git fetch github
   git rebase <vma-branch-name>
   ```

3. 解决冲突时注意：
   - slab 代码只应改动 `runtime-heap-allocator`、`main.rs` 的 frame source；
   - VMA 分支改动集中在 `wateros-mm/mm-impl/*`；
   - 不要接受任一侧盲目覆盖。
4. 重新构建：

   ```bash
   cd /tmp/wateros-kernel-heap-slab/os
   make rv_check
   make la_check
   HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-rv-final
   HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-la-final
   ```

5. 先做 RV/LA 单核功能，再做 LA 12 核、RV 8 核完整 BuildStorm；
6. 确认 `elf-lazy-map` 已恢复默认开启后，再测纯 slab 性能。

## 不要做的事

- 不要删除本工作树；
- 不要手动 `reset --hard`；
- 不要在 VMA 分支混入 slab 代码，保持两个分支独立。
