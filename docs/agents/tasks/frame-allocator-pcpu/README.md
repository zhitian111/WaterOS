# frame-allocator-pcpu 任务

本目录是 `perf/frame-allocator-pcpu` 分支的专用任务拆解，目标是在已完成的
`perf/kernel-heap-slab` 之上，把 slab 补页路径从“每页抢全局 frame allocator
锁”升级为 Linux 风格的“per-CPU frame batch cache + 全局慢路径批量补充”。

## 分支与工作树

- 分支：`perf/frame-allocator-pcpu`
- 工作树：`/tmp/wateros-frame-allocator-pcpu`
- 基线：`perf/kernel-heap-slab` 的 slab 验收结果
- CodeGraph：完成 `codegraph init` 后，本目录优先使用 `codegraph explore`
  / `codegraph impact` / `codegraph callers` 定位调用链。

## 目标

保持 Linux 语义不失效，进一步降低 BuildStorm 中内核堆/页分配路径的全局锁
频率，使最终性能相对 slab 基线有可测量提升。

## 预期改造路径

```text
per-CPU slab object cache
        |
        v
per-CPU frame batch cache          <- 本任务新增
        |
        v
global frame allocator / buddy     <- 仅在 batch 耗尽时批量进入
```

全局 frame allocator 仍然存在，但每次补充从“拿 1 页锁一次”变成“拿 N 页锁一次”。

## 验收主线

### 功能 / bug 验收

1. 两架构都使用 QEMU 9.2.1、线上等价参数连续 3 轮全新镜像完整 BuildStorm；
2. 每轮满足：

   ```text
   TOOLCHAIN_RESULT status=OK
   MINIBUILD_RESULT status=OK
   BUILDSTORM_RESULT mode=multi status=OK rc=0 ... run=OK
   WaterOS: all commands finished
   ```

3. 无 panic、OOM、ENOMEM、SIGSEGV/fault、`recursive heap allocation`、
   `shootdown timeout` 等异常。

### 性能验收

- 基线：本分支起点 `perf/kernel-heap-slab` 在相同配置下跑出的中位数。
- 最终：本分支在相同脚本、镜像、QEMU 参数下跑 3 轮，取中位数。
- 要求：RV 和 LA 最终中位数都小于 slab 基线中位数；过程中允许回退，最终必须
  优于基线。
- 性能测试前必须确认系统中没有其他 `qemu-system-*` 进程；否则等待退出。

## 镜像准备与 QEMU

所有镜像解压到 `~/Downloads`，所有 QEMU 运行加 `-snapshot`，使用
`/home/zhitian/qemu_9_2_1/qemu-9.2.1/build` 下的 QEMU。

镜像脚本覆写与启动命令沿用 `perf/kernel-heap-slab` 的 task 文档。

## 任务顺序

| 任务 | 目标 |
|---|---|
| `00-baseline-and-profiling.md` | 记录 slab 基线并加低开销页/remote 统计 |
| `01-frame-batch-api.md` | frame allocator 增加 per-CPU batch refill/release 语义 |
| `02-slab-integration.md` | slab 补页改走 per-CPU frame batch |
| `03-low-risk-fastpaths.md` | size class 查找与 remote-free 等低风险热路径优化 |
| `04-functional-acceptance.md` | 双架构完整功能回归 |
| `05-performance-acceptance.md` | 双架构 3 轮中位数验收与收尾 |

## 任务简报

每个任务完成后新增：

```text
docs/agents/tasks/frame-allocator-pcpu/history/<task-id>-brief.md
```

内容至少包括完成情况、改动文件、验收命令和结果、未验证项、文档同步清单。

