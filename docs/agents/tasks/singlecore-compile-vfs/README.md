# singlecore-compile-vfs 任务

本目录是 `perf/singlecore-compile-vfs` 分支的专用任务拆解，目标是在
`perf/frame-allocator-pcpu` 基础上，进一步优化 BuildStorm 中单核
`rustc/cargo` 串行编译阶段。

## 分支与工作树

- 分支：`perf/singlecore-compile-vfs`
- 工作树：`/tmp/wateros-singlecore-compile-vfs`
- 基线：`perf/frame-allocator-pcpu`
- 基线内核保存在：

  ```text
  /home/zhitian/project/WaterOS_refactor/os/.perf-baseline-pre-singlecore/
  ```

- CodeGraph：已初始化，优先使用 `codegraph explore` / `impact` / `callers`。

## 目标

保持 Linux 语义，优先降低单核编译阶段的 VFS 路径解析、元数据查询、顺序文件读取
和短进程启动成本。

## 验收主线

### 功能 / bug 验收

1. 双架构使用 QEMU 9.2.1、线上等价参数连续 3 轮全新镜像 BuildStorm；
2. 每轮满足 TOOLCHAIN/MINIBUILD/BUILDSTORM OK；
3. 无 panic/OOM/ENOMEM/SIGSEGV/fault/文件缓存失效异常。

### 性能验收

- 基线：`perf/frame-allocator-pcpu` 对应 16G 配置中位数。
- 最终：本分支同样配置 3 轮取中位数。
- 要求 RV 和 LA 最终中位数都小于基线中位数。
- 性能测试前必须等待系统内其他 QEMU 退出。

## 任务顺序

| 任务 | 目标 |
|---|---|
| `00-baseline-and-invariants.md` | 固化 frame 分支基线并梳理缓存失效不变量 |
| `01-vfs-dcache.md` | 实现 VFS 目录项/负目录项缓存核心 |
| `02-statx-openat-hit.md` | statx/openat/readlinkat 共享 dcache 命中路径 |
| `03-sequential-readahead.md` | paged_handle 顺序读预取 |
| `04-fork-exec-shortpath.md` | 短进程 fork/exec 低风险优化 |
| `05-futex-clock-fastpath.md` | futex/clock_gettime 无竞争快路径 |
| `06-final-acceptance.md` | 双架构功能/性能终验与收尾 |

## 任务简报

每个任务完成后新增：

```text
docs/agents/tasks/singlecore-compile-vfs/history/<task-id>-brief.md
```

