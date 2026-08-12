# 性能优化 Agent 任务索引

[项目首页](../../../README.md) · [文档总览](../../README.md) · [任务总览](../README.md)

本目录提供**可直接 @ 下发给 Agent** 的性能优化任务。每条任务自包含背景、源码定位、验收标准与一次性 prompt 模板。

## 评分背景（所有任务共用）

- 性能项 `score = max(1.0, 优于 baseline 的程度)`；**刚好到 baseline 不加分**。
- `score = 0` = 失败/超时/非法值（修就有分）。
- 分析依据：`docs/todo/perf-baseline-gap-report.md`、`docs/todo/perf-fork-exit-degradation.md`、各 `docs/todo/perf-*.md`。

## 使用方式

向 Agent **只 @ 一个任务文件**即可，例如：

```
@docs/tasks/perf/wave1-enable-block-cache/task.md

请按任务文件执行。
```

任务文件内已列出须阅读的 prompt、分析文档与源文件；Agent 应先读清单再改代码。

## 共同必读

| 类型 | 路径 |
|------|------|
| prompt | `docs/agents/prompts/general.md`、`structure.md`、`coding.md`、`architecture.md` |
| 得分缺口 | `docs/todo/perf-baseline-gap-report.md` |
| 风险分级 | `docs/todo/perf-risk-assessment.md` |
| 构建验证 | `docs/agents/tasks/run_testsuits_qemu.md`（定向 benchmark 阶段） |

涉及子系统时再读对应源码与 `docs/todo/perf-<subsystem>.md`。

## 任务列表（建议执行顺序）

### 第 0 波：合入已有 worktree（可选）

| 任务文件 | 目标 | 风险 |
|----------|------|------|
| [`wave0-merge-low-risk-worktree.md`](./wave0-merge-low-risk-worktree/task.md) | 合入 `perf-low-risk-8d52acf0` 已落地的 11 项 | 低 |

### 第 1 波：低风险、高确定性拿分

| 任务文件 | 缺口 | 预期收益 |
|----------|------|----------|
| [`wave1-enable-block-cache.md`](./wave1-enable-block-cache/task.md) | G2a/b：RV/LA 块缓存未启用、容量过小 | iozone 读/写翻线 |
| [`wave1-fix-scheduler-versions-leak.md`](./wave1-fix-scheduler-versions-leak/task.md) | D2：`OtherReadyQueue.versions` 泄漏 | fork/exit 稳定性、调度 |
| [`wave1-fix-ctx-switch-zero-score.md`](./wave1-fix-ctx-switch-zero-score/task.md) | G3：lmbench ctx switch 计 0 | +10~14 分 |
| [`wave1-fix-functional-zero-score.md`](./wave1-fix-functional-zero-score/task.md) | G4/G5/G9：regex、Pagefaults、busybox | +10~14 分 |

### 第 2 波：中风险、翻 baseline

| 任务文件 | 缺口 | 预期收益 |
|----------|------|----------|
| [`wave2-fs-read-path.md`](./wave2-fs-read-path/task.md) | G2c~f：dcache、512B 分片、页缓存 LRU、预取 | iozone 读、lmbench stat/open |
| [`wave2-execve-lazy-map.md`](./wave2-execve-lazy-map/task.md) | fork+/bin/sh 920ms | lmbench Process、shell 启动 |
| [`wave2-network-throughput.md`](./wave2-network-throughput/task.md) | G8：iperf/netperf 卡 1.0 | 网络项翻线 |

### 第 3 波：高风险（必须 Feature Flag）

| 任务文件 | 缺口 | 预期收益 |
|----------|------|----------|
| [`wave3-kernel-heap-allocator.md`](./wave3-kernel-heap-allocator/task.md) | D1：linked_list 堆碎片化 | fork/exit 骤降根因 |
| [`wave3-trap-tlb-hotpath.md`](./wave3-trap-tlb-hotpath/task.md) | G7：syscall/stat 延迟 | lmbench 多项翻线 |
| [`wave3-fork-exit-deep-opt.md`](./wave3-fork-exit-deep-opt/task.md) | M-3/L-1/D3：页表 COW、reap、PID | fork+exit、ctx setup |

## 不在此目录的任务

- **LA-musl LTP 整套 0 分（G1）**：功能性，由用户自行修复；见 `docs/todo/perf-baseline-gap-report.md` §G1。
- **全量性能分析只读**：见 `docs/todo/` 下各 `perf-*.md`，不单独建实施任务。

## 2026-08-08 新增记录

- [`page-cache 索引清理改用 BTree range`](./wave2-fs-read-path/history/2026-08-08-page-cache-index-range.md)：
  消除页缓存 `purge/rename/truncate` 的全表键扫描。
- [`page-cache 规范化 key A/B`](./wave2-fs-read-path/history/2026-08-08-page-cache-key-canonical-ab.md)：
  实验显示 `memcmp` 下降但总指令上升，已回退。
- [`路径规范化快速路径 A/B`](./wave2-fs-read-path/history/2026-08-08-path-normalize-fast-ab.md)：
  快速路径未降低目标符号，总指令上升，已回退。

## 2026-08-09 新增记录

- [`页缓存稳定 node key`](./wave2-fs-read-path/history/2026-08-09-page-cache-stable-node-key.md)：
  稳定文件改用数字 node id 比较，`memcmp` 下降约 76%，RV/LA 完整 Final 通过。
- [`TLSF 懒统计 A/B`](./wave3-kernel-heap-allocator/history/2026-08-09-tlsf-stats-lazy-rejected.md)：
  减少 allocator 统计读取未带来收益，总指令上升，已回退。
- [`块缓存 16MiB A/B`](./wave1-enable-block-cache/history/2026-08-09-block-cache-16m-ab.md)：
  VirtIO 下降但总指令上升，已回退到 8MiB。

## 验证约定

- 改代码后：`cd os && make rv_check && make la_check`（两架构均须通过）。
- 性能回归：P3 benchmark（lmbench/iozone）或 P4 网络，见 `run_testsuits_qemu.md`；勿在一次 QEMU 里开全阶段。
- 功能回归：至少 P1 basic + P2 busybox；动 FS/进程/网络 时加对应阶段。
- **不要**把 `os/log`、`/tmp/*.log`、评测 `score.txt` 提交进 git。
