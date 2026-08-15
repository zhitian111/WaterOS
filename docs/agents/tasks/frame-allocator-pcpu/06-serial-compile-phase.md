# Task 06：后续分析 BuildStorm 串行编译阶段

## 背景

BuildStorm 的可并行编译阶段已经表现较快，但若干 `Compiling ...` crate 会长时间
只在单核运行，例如 `uefi`、`unwind`、`hashbrown`、`ax_posix_api` 等。这些串行
阶段可能是下一轮性能优化的重点。

## 任务目标

在 frame allocator per-CPU batch 优化完成后，分析这些串行编译阶段为什么慢，
并找到 WaterOS 内核可优化点；不改用户镜像中的 workload。

## 初步排查方向

1. 是否是单线程 rustc 编译本身，不构成内核优化机会；
2. 是否大量小文件读取、`lseek`、`stat`、`read` 或路径解析串行化；
3. 是否 page cache/块缓存命中不足，导致单核编译频繁回源；
4. 是否 fork/exec/进程回收路径造成单核停顿；
5. 是否 VFS 路径解析或 ext4 索引锁导致单个编译任务无法并行；
6. 是否调度器把可运行任务错误绑在单个 CPU 上。

## 建议工具

- 用 `strace`/`perf` 或内核低开销 counter 记录单核编译阶段的 syscall 分布；
- 解析 BuildStorm 日志中 `Compiling <crate>` 时间戳，确认长尾 crate；
- 对特定 crate 做单核/多核 A/B。

## 验收方式

先出分析报告，不直接改代码；若确认内核瓶颈，再拆成新的 commit 级任务。

## 完成后

新增 `history/06-brief.md`。
