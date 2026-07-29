# WaterOS 已知问题任务索引

## 范围与口径

本目录是当前问题的统一执行入口，不按成员划分。状态依据 2026-07-29 的 `main`
（`b255b18e`）、工作树源码和现有运行报告整理。旧报告中的结论若已被代码实现取代，
标为“待复验”，不能再次照旧实现。

任务分为三类：

- **确认未闭环**：已有源码或最新日志证据，直接执行。
- **已实现待复验**：先跑指定测试；只有仍失败才改代码。
- **测量后候选**：没有阶段 1 数据不得合入策略性优化。

所有任务必须先读 `docs/prompts/general.md`、`docs/prompts/structure.md`、
`docs/prompts/coding.md` 和 `docs/prompts/architecture.md`。公共契约放 `api-v0`，
机制放 `impl-*`，平台特殊逻辑留在 `platform-impl`，架构通用 trap/启动逻辑留在
`platform-arch`。不得顺手修改 `os/vendor/`。

## 当前事实

- CAgent 已有连续三轮 10/10 记录，但最终候选版本仍须复验。
- Cargo 大索引读取根因已确认：大于 4 MiB 的 `read(2)` 被错误拒绝；短读修复尚在
  当前工作树，完整 BuildStorm 仍未输出 `BUILDSTORM_COMPILE mode=multi ok=true`。
- another-ext4 已通过基本读写和干净镜像完整性检查；压力场景仍需用写回、rename、
  unlink、truncate 与掉电边界测试证明。
- 双架构已有 SMP/IPI 实现：RISC-V 使用 SBI HSM/IPI，LoongArch 使用 IOCSR。
  “没有 IPI 实现”是过期结论；双架构 8 核运行门禁仍未完整记录。
- block cache 已在 RV/LA feature 中启用，容量为 1024 块；网络 RX/TX 已扩到 64 KiB
  且支持批量收包；ELF lazy map 和 TLSF 可选后端也已存在。这些任务从测量开始。

## 必须同做与并行关系

```text
K-01 BuildStorm + fsync + 写回一致性 ─┐
RIO-01..10 read 调用族 ───────────────┼─> K-04 固定基线与测量
K-02 双架构 SMP/LA-musl ──────────────┤
K-03 功能性 0 分项 ──────────────────┘

K-04 ─┬─> K-05 FS/VFS 性能
      ├─> K-06 task/scheduler/futex
      ├─> K-07 MM/exec/fork/heap
      └─> K-08 网络吞吐

K-04 + K-06 + K-07 ─> K-09 trap/TLB 高风险优化
全部保留改动 ───────> K-10 最终回归与交付
```

K-01 内的 `fsync`、page-cache writeback、unlink/rename 失效和 another-ext4 flush 使用
同一持久化契约，必须作为一个闭环设计。RIO 内部依赖见
[`read-family/README.md`](../read-family/README.md)。K-05 至 K-08 可并行，但阶段 1
最多选择三个有数据支持的瓶颈。

## 任务清单

| 状态 | 任务 | 类型 | 交付 |
|---|---|---|---|
| [ ] | [`K-01`](./01-buildstorm-fs-durability.md) | 确认未闭环 | BuildStorm、fsync、时间戳、写回一致性 |
| [ ] | [`RIO-01..10`](../read-family/README.md) | 确认未闭环 | read/readv/pread、MM copy、OFD、读取源 |
| [ ] | [`K-02`](./02-smp-loongarch-validation.md) | 待复验 | RV/LA 8 核、IPI、LA-musl LTP |
| [ ] | [`K-03`](./03-functional-zero-scores.md) | 待复验 | regex、Pagefaults、busybox 0 分项 |
| [ ] | [`K-04`](./04-baseline-and-instrumentation.md) | 确认未完成 | Linux/WaterOS 三轮基线与 Top 3 |
| [ ] | [`K-05`](./05-fs-vfs-performance.md) | 测量后候选 | dcache、页缓存 LRU、读写放大 |
| [ ] | [`K-06`](./06-task-scheduler-futex.md) | 测量后候选 | ctx、队列、futex、退出生命周期 |
| [ ] | [`K-07`](./07-mm-exec-fork-heap.md) | 测量后候选 | lazy ELF、fork/COW、回收、allocator |
| [ ] | [`K-08`](./08-network-throughput.md) | 测量后候选 | poll、锁、收发批处理、缓冲 |
| [ ] | [`K-09`](./09-trap-tlb-hotpath.md) | 高风险候选 | trap、TLB/ASID、user-copy walk |
| [ ] | [`K-10`](./10-final-regression-delivery.md) | 最终门禁 | 双架构功能、性能、镜像与文档 |

K-02、K-03、K-05、K-06、K-07 是共享契约和合并门禁，实际分派使用下列可并行 leaf：

| 并行组 | 独立任务文件 |
|---|---|
| K-02 | [`02A SMP/IPI`](./02a-smp-ipi-runtime.md)、[`02B LA-musl`](./02b-loongarch-musl-ltp.md) |
| K-03 | [`03A regex`](./03a-regex-zero-score.md)、[`03B Pagefaults`](./03b-musl-rv-pagefault.md)、[`03C busybox`](./03c-busybox-kill-mv-rmdir.md) |
| K-05 | [`05A inode/dcache`](./05a-inode-dentry-cache.md)、[`05B page LRU`](./05b-page-cache-lru.md)、[`05C I/O/prefetch`](./05c-io-merge-prefetch.md) |
| K-06 | [`06A scheduler`](./06a-scheduler-ctx.md)、[`06B futex`](./06b-futex-waitqueue.md)、[`06C reap`](./06c-process-reap-lifecycle.md) |
| K-07 | [`07A lazy ELF`](./07a-elf-lazy-map.md)、[`07B fork/page table`](./07b-fork-pagetable-lifecycle.md)、[`07C heap`](./07c-kernel-heap-backend.md) |

K-07B 依赖 K-06C 的 retired-process 接口，不能同时修改生命周期 API；K-05A 与 K-05B
可并行，但必须先冻结 file identity/cache key。其余同一行 leaf 可使用独立 worktree
并行，最终按父任务验收合并。

## 完整问题映射

| 既有编号/来源 | 新入口 |
|---|---|
| BuildStorm 完整编译、`fsync`、时间戳、F-2/F-6/F-7 | K-01 |
| read-family R1-R13、I-6、部分 L-3 | RIO-01..10 |
| 决赛 SMP、OpenSBI/IOCSR、G1 LA-musl LTP | K-02 |
| G4 regex、G5 Pagefaults、G9 kill/mv/rmdir | K-03 |
| 阶段 1 计数器和可比基线 | K-04 |
| G2/G6、F-1..F-21 中尚有数据支持的项 | K-05 |
| G3、I-1..I-17、L-1..L-20 的 task/IPC 部分 | K-06 |
| M-3..M-20、fork/exec/heap、L-1 的 MM 部分 | K-07 |
| G8 与 network/poll 相关项 | K-08 |
| G7、H-1/H-2/H-4/H-6、M-1/M-2 | K-09 |
| 阶段 3、CAgent、BuildStorm、LTP、文件系统完整性 | K-10 |

未被阶段 1 选中的 H/M/F/I/L 长尾项继续保留在 `docs/todo/perf-*.md`，不视为已经
批准实施。发现新问题时先补最新日志、最小复现和依赖边，再决定归入现有任务或新增
任务，不能直接替换当前 P0。

## 通用验收规则

- 构建至少执行 `cd os && make rv_check && make la_check`。
- 行为变更须在 RISC-V64 和 LoongArch64 验证；确实缺镜像时记录为阻断，不能用另一
  架构代替。
- 每轮 QEMU 使用独立 qcow2 overlay，原始镜像前后 SHA-256 不变。
- FS 写测试结束后将 overlay 转 raw，运行 `e2fsck -fn` 并保留五阶段结果。
- 性能数据至少三轮，保留单轮值、中位数、QEMU 参数、commit 和原始日志路径。
- 临时高频日志、kernel、overlay、镜像和大日志不得提交。
