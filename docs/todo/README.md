# WaterOS 内核性能优化分析（待办）

## 用途

本目录汇总对 WaterOS 内核**性能优化点**的系统化分析，覆盖热调用路径、资源回收与 flush、锁竞争三大方向，并为每一条改进点提供：精确源码 `文件:行号`、当前算法与复杂度、问题成因、改进方案、预期收益、双架构差异与风险。

**实施进度**：[`perf-risk-assessment.md`](./perf-risk-assessment.md) 第 1 层低风险共 11 项（H-3 覆盖 H-10）已于 2026-06-28 在 worktree `perf-low-risk-8d52acf0` 完成代码落地与 QEMU 回归；**尚未合入主仓库**。第 2 层及以下仍为待办分析与方案。

> **最新（v2，数据驱动）**：[`perf-baseline-gap-report.md`](./perf-baseline-gap-report.md) 基于评测平台 `score.txt` 实测结果，分析「为什么大量样例只到 baseline（score=1.0）而拿不到额外分」。**先看这份**——它给出了评分规则（`score = max(1.0, 比值)`，接近 baseline 不得分）、按可恢复分值排序的得分缺口地图（含 **LA-musl LTP 整套 0 分 ≈ 568 分** 的最大缺口、**块缓存在两架构均未启用**、**context switch 计 0** 等关键事实）与翻线优先级。本目录其余文档（按子系统枚举热点）作为其交叉索引。

> **Agent 实施任务**：可直接 @ 下发的提示词见 [`docs/tasks/perf/README.md`](../tasks/perf/README.md)（按 wave0~3 分文件，含一次性 prompt 模板）。

## 事实来源

- **代码静态链路分析**（riscv64 + loongarch64 双架构），由多个子任务并行完成：
  - 热路径 [hotpath-subagent](48f8b89e-5c0e-4728-9bd7-2c4b04f26840)
  - 内存 [memory-subagent](09ce5359-c553-46ad-8db4-30888ce225e1)
  - FS/VFS [fs-vfs-subagent](fcb92735-08b2-4ca4-9db7-9f165361f9f5)
  - IPC [ipc-subagent](0977065a-2981-472f-97fd-053c931ade50)
  - 锁与回收 [lock-resource-subagent](2dacab6d-5c9f-4263-99c2-dd4839a74bd6)
- **日志佐证（用于确认高频路径，非性能 profiling）**：`os/ltp_log/rv_ltp_glibc_local_all.log`、`rv_ltp_musl_local_all.log`、`la_ltp_glibc_local_all.log`（含 `la_ltp_glibc_rerun*`、`la_ltp_musl_rerun*` 续轮）、`os/la_local_run_all.log`、`os/rv_local_run_all.log`。这些是 LTP/busybox 功能 trace 日志，关键证据：`[syscall]`/`[trap]`/`[exit]` 标签密集、`mount`/`ioctl`/`clone` unsupported 警告 1500+ 条、121 次 `[exit] clear_child_tid write failed`、`[paged_handle] detached buffer cap exceeded`，说明 syscall 分发、trap 返回、进程退出、整文件读堆等路径被高频触发。
- **复用已有审计**：`docs/audits/lock-inventory.md`、`docs/audits/resource-inventory.md` 及其子文档（`docs/audits/locks/*`、`docs/audits/resources/*`）。

> 重要说明：日志不含真实计时数据，所有「热点 / 复杂度」结论均来自代码链路分析，日志仅用于佐证路径被高频触发。文中关键行号已抽查核实（如页缓存 LRU 线性扫描、帧分配器 `recycled.contains` 冗余 O(n)）。实施前仍应按 `docs/prompts/general.md` 再次 `grep`/读文件确认。

## 覆盖范围与文档索引

| 文档 | 子系统 | 条目数 | 核心主题 |
|------|--------|--------|----------|
| [`perf-hotpath.md`](./perf-hotpath.md) | syscall 分发 / trap / 上下文切换 / 调度 | H-1~H-16 | trap 往返税、用户拷贝 walk、dispatch 跳表、TLB/ASID、等待队列索引 |
| [`perf-memory.md`](./perf-memory.md) | 帧分配 / 页表 / 内核堆 / mmap / COW / TLB | M-1~M-20 | 全局 TLB flush、fork 页表复制、destroy/munmap 帧回收、brk 懒加载 |
| [`perf-fs-vfs.md`](./perf-fs-vfs.md) | 页缓存 / 块缓存 / ext4 / flush / 回收 | F-1~F-21 | O(1) LRU、unlink/sync 丢脏、整文件读堆、dcache、LA 块缓存缺失 |
| [`perf-ipc-sync.md`](./perf-ipc-sync.md) | futex / pipe / signal / waitqueue / shm / poll·epoll | I-1~I-17 | epoll 事件驱动、futex requeue 释锁、exit 队列回收、惊群、signal 快表 |
| [`perf-lock-resource.md`](./perf-lock-resource.md) | 进程退出 / fork / fd 表 / 注册表 | L-1~L-20 | reap 锁外 drop、fd 表 O(N²)、注册表反向索引、exit_group 合并 |
| [`perf-risk-assessment.md`](./perf-risk-assessment.md) | 风险收益评估 + 安全实施 | 全部 | 风险×收益矩阵、项目特有风险、Flag/验证、排期建议 |

每个子系统文档末尾均有「风险与验证速查」表（本文件「风险×收益」矩阵与 `perf-risk-assessment.md` 的内联子集）。

## 跨子系统优先级矩阵（Top 改进点）

### P0 — 高收益且影响面广（建议优先）

| 编号 | 标题 | 文档 | 类型 |
|------|------|------|------|
| M-1 / H-1 / H-4 | 全局 TLB flush + RV trap 往返多重拷贝（ASID 未利用） | memory / hotpath | 热路径 |
| H-2 | 用户内存拷贝每页重复软件 walk、路径串逐字节 | hotpath | 热路径 |
| H-3 | syscall 分发双重 decode（无跳表） | hotpath | 热路径 |
| M-3 / M-4 / M-5 | fork 页表全复制、destroy 逐表 512 扫描、munmap 不回收中间帧 | memory | 回收 |
| F-2 / F-7 / F-6 | unlink/sync 丢脏、mount alias bump 不 flush | fs-vfs | flush 正确性 |
| F-4 / F-8 | 页缓存 LRU O(n) 线性扫描 + miss 堆分配/clone | fs-vfs | 热路径 |
| I-1 / I-17 | poll/select O(nfds) + epoll 缺失 | ipc | 事件驱动 |
| I-2 / I-3 | futex requeue 持锁唤醒 + exit 不回收空队列 | ipc | 锁/回收 |
| L-1 | 进程 reap 持 Registry 锁内销毁地址空间 | lock-resource | 锁/回收 |

### P1 — 高收益但范围较局部

| 编号 | 标题 | 文档 |
|------|------|------|
| F-1 / F-15 | AuxRo/ext4 整文件读入堆，未用 read_range | fs-vfs |
| F-3 | ext4 每次 I/O 全路径 path_to_inode，无 dcache | fs-vfs |
| F-9 | LoongArch 未启用块缓存 | fs-vfs |
| M-6 / M-19 | brk/栈/共享 anon eager 清零，brk 未 lazy | memory |
| L-3 / L-4 / L-5 / L-6 | fork fd duplicate、unix_sock 全表扫描、Registry 线性查找、alloc_fd O(N²) | lock-resource |
| L-2 / L-7 | 线程 exit 多锁串行、exit_group 重复清理 | lock-resource |
| H-6 / I-8 | 每次 syscall 返回查 pending signal（TCB 快表） | hotpath / ipc |

### P2 — 中收益 / 特定场景 / quick win

H-5、H-7~H-13、M-7~M-17、M-20、F-5、F-10~F-18、I-5~I-16、L-8~L-15。

### 正确性优先（性能收益低但属 P0 数据/资源正确性）

| 编号 | 标题 | 文档 |
|------|------|------|
| M-18 | MAP_SHARED 匿名 fork 不 inc_ref / destroy 不回收 | memory |
| F-2 | unlink 丢脏页 | fs-vfs |
| I-10 / I-12 | SHM fork 失败不回滚 / futex WAIT 无 alternate key 致永久睡眠 | ipc |

## 风险 × 收益矩阵（能否不引入 bug？）

这批改进点风险跨度很大，**收益最高的几项往往最危险**（静默/非确定性错误）。完整口径、项目特有风险与逐条评估见 [`perf-risk-assessment.md`](./perf-risk-assessment.md)。下表给出二维分布，建议按「左上 → 右上」排期。

| 收益 ＼ 风险 | 低风险（行为保持，可安全做） | 中 / 中高（配断言/Flag/定向测例） | 高（静默错误，须 Flag+灰度+增量） |
|------|------|------|------|
| **高** | H-3、F-9、F-4✚、M-8 | F-2★、F-6★、F-7★、F-3、F-5、F-8、L-4✚、L-5✚、L-6✚、L-1、L-2、L-3、L-7、I-1、I-2、I-3✚、I-4、I-17、H-2 | M-1、M-2、M-3、M-5、H-1、H-4 |
| **中** | H-10、F-14 | M-6、M-7、M-9、M-19、M-20、F-1、F-10~F-13、F-15~F-18、I-5~I-12★、I-14、L-8~L-15、H-5~H-9、H-15 | H-14、M-16 |
| **低** | M-17、I-13、I-15、F-20、F-21、L-17、H-16 | M-11~M-14、F-19、L-18~L-20、I-16 | — |

> 图例：`★`=本质是修现存 bug（做了净减风险）；`✚`=建议同时加 debug 断言守住数据结构/索引不变量。

**一句话回答**：低风险层（行为保持型）可以在现成 LTP+busybox 回归兜底下基本安全完成；中高层需配断言、Flag 与定向测例才能控住；高风险层（TLB/ASID、底层 trap、页表 COW、lazy FPU）出 bug 往往静默且非确定性，必须 Flag 化、灰度、增量验证，**不可一次性大重构**。

## 三大需求方向归类（对应用户关注点）

- **Hot 调用路径**：H-1~H-5、H-15（trap/dispatch/拷贝/fault）；M-1、M-9、M-10；F-3、F-4、F-8；I-1、I-6、I-8；L-5、L-6。
- **资源回收**：M-2~M-5、M-12、M-18；F-2、F-10、F-19；I-3、I-9、I-11；L-1~L-7、L-14~L-20。
- **Flush / 写回**：F-2、F-5、F-6、F-7、F-13；L-11；H-1、M-1（TLB flush）。

## 双架构差异要点

- **RISC-V**：trampoline 双拷贝 + 多次 `sfence.vma` 全局 flush 是相对 LA 最突出的额外 trap 税（H-1、M-1）；已分配 ASID 但未用于 selective flush（M-2）；有 64 槽块缓存（F-13）。
- **LoongArch64**：trap 入口更轻（直接在内核栈建帧），但无 ASID、始终全局 `invtlb`，多地址空间场景 TLB 压力更大（M-1）；**未启用块缓存**（F-9），ext4 I/O 性能落后 RV。
- syscall 分发、用户拷贝软件 walk、wait queue 扫描、页缓存/帧分配器算法在两架构**同源**。

## 后续维护入口

- 实施任一改进点前，先读 `docs/prompts/general.md`「构建与运行」，编码后用 `cd os && make rv_check` / `make la_check` 验证，运行行为用 `make rv_qemu_run` / `make la_qemu_run`。
- 改动涉及的同步文档见各子文档末尾「后续维护入口」与 `docs/prompts/structure.md`「重要同步文件」。
- 本目录文档随实施进度更新条目状态；**第 1 层完成态与实测时间说明**见 [`perf-risk-assessment.md`「第 1 层实施状态」](./perf-risk-assessment.md#第-1-层实施状态2026-06-28)。与功能修复任务（`os/ltp_log/todo/*`）正交，但 epoll（I-1/I-17）、vfs_io flush（F-7）等存在交集，实施时合并跟踪。
